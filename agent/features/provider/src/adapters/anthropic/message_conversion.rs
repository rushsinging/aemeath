//! Anthropic message conversion and non-streaming fallback helpers

use reqwest::header::HeaderMap;
use share::message::{ContentBlock, Message, Role};
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
    HttpFailureKind, SuccessBodyReadError,
};
use crate::domain::invoke::{CreateMessageRequest, StopReason, StreamResponse, SystemBlock, Usage};
use crate::ports::LegacyStreamSink;

// ---------------------------------------------------------------------------
// Tool schema sanitize — strip internal-only fields (data_schema etc.)
// before sending to the Anthropic Messages API. Only spec-allowed keys
// survive: name, description, input_schema, cache_control, type.
// ---------------------------------------------------------------------------

/// Anthropic Messages API tool spec 允许的字段白名单。
const ANTHROPIC_TOOL_ALLOWED_KEYS: &[&str] = &[
    "name",
    "description",
    "input_schema",
    "cache_control",
    "type",
];

/// 将内部 tool schema（含 `data_schema` 等扩展字段）清洗为 Anthropic
/// Messages API 兼容格式，只保留白名单字段。
pub(crate) fn sanitize_tool_schemas(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let empty = serde_json::Map::new();
    tool_schemas
        .iter()
        .map(|schema| {
            let obj = schema.as_object().unwrap_or(&empty);
            let filtered: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .filter(|(k, _)| ANTHROPIC_TOOL_ALLOWED_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(filtered)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Message conversion — explicitly build Anthropic Messages API JSON from
// internal Message, avoiding serde_json::to_value leaks (metadata,
// placeholder, text etc. are internal-only and must never reach the wire).
// ---------------------------------------------------------------------------

/// 将内部 `Message` 列表转换为 Anthropic Messages API 兼容的 JSON 数组。
///
/// 显式遍历每个 `ContentBlock`，按 API 规范构建 wire format，丢弃所有
/// 内部扩展字段（`metadata`、`Image.placeholder`、`ToolResult.text`）。
/// 与 OpenAI 兼容 driver 的 `convert_messages` 对齐——各 driver 负责自己
/// 的 wire format。
pub(crate) fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    messages.iter().map(convert_message).collect()
}

/// 在 messages 倒数第二条消息的最后一个 content block 上注入
/// `cache_control: {"type": "ephemeral"}` 断点，让 Anthropic 缓存
/// 整个对话历史前缀。
///
/// Agentic loop 每 turn 新增 2 条消息，penultimate 消息不断前移，
/// 使前一 turn 的缓存前缀与当前 turn 的请求前缀匹配 → cache hit。
///
/// 配合 system static block（断点①）和 tools 数组（断点②），
/// 共使用 3/4 个 Anthropic 允许的 cache_control 断点。
pub(crate) fn apply_message_cache_breakpoint(messages: &mut [serde_json::Value]) {
    if messages.len() < 2 {
        return;
    }
    let penultimate = messages.len() - 2;
    if let Some(content) = messages[penultimate]
        .get_mut("content")
        .and_then(|c| c.as_array_mut())
    {
        // 倒序查找最后一个非 thinking block（Anthropic 不允许在 thinking
        // content block 上设置 cache_control，返回 400）
        for block in content.iter_mut().rev() {
            let is_thinking = block
                .get("type")
                .and_then(|t| t.as_str())
                .map(|s| s == "thinking" || s == "redacted_thinking")
                .unwrap_or(false);
            if !is_thinking {
                if let Some(obj) = block.as_object_mut() {
                    obj.insert(
                        "cache_control".to_string(),
                        serde_json::json!({"type": "ephemeral"}),
                    );
                }
                break;
            }
        }
    }
}

fn convert_message(msg: &Message) -> serde_json::Value {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<serde_json::Value> = msg
        .content
        .iter()
        .map(convert_block)
        .filter(|v| !v.is_null())
        .collect();
    serde_json::json!({
        "role": role,
        "content": content,
    })
}

fn convert_block(block: &ContentBlock) -> serde_json::Value {
    match block {
        ContentBlock::Text { text } => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        ContentBlock::Image { source, .. } => {
            // 丢弃 placeholder（内部 round-trip 字段，不发给 LLM）
            match source {
                share::message::ImageSource::Base64 { media_type, data } => serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": data,
                    },
                }),
            }
        }
        ContentBlock::ToolUse { id, name, input } => serde_json::json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            text,
        } => {
            // Anthropic 仅接受字符串或 content block 数组；优先使用 text-first 输出，
            // 并将旧会话中遗留的对象/标量 JSON 序列化为文本，避免非法请求出站。
            let content = text
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or_else(|| {
                    if content.is_object()
                        || content.is_number()
                        || content.is_boolean()
                        || content.is_null()
                    {
                        serde_json::Value::String(content.to_string())
                    } else {
                        content.clone()
                    }
                });
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error,
            })
        }
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            // Anthropic 要求后续请求中 thinking block 必须带 signature，
            // 否则返回 400 (`thinking.signature: Field required`)。
            // 有 signature → 回传；无 signature（旧 session / 非 Anthropic 来源）→ 剥离。
            match signature {
                Some(sig) => serde_json::json!({
                    "type": "thinking",
                    "thinking": thinking,
                    "signature": sig,
                }),
                None => serde_json::Value::Null,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

/// Handler wrapper that tracks whether any user-visible content was emitted.
/// Used to decide if a non-stream fallback is safe on stream errors — if any
/// text/tool_use was already shown, falling back would duplicate it.
pub(crate) struct TrackingHandler<'a> {
    pub(crate) inner: &'a mut dyn LegacyStreamSink,
    pub(crate) emitted: bool,
}

impl<'a> TrackingHandler<'a> {
    pub(crate) fn new(inner: &'a mut dyn LegacyStreamSink) -> Self {
        Self {
            inner,
            emitted: false,
        }
    }
}

impl<'a> LegacyStreamSink for TrackingHandler<'a> {
    fn on_text(&mut self, text: &str) {
        self.emitted = true;
        self.inner.on_text(text);
    }
    fn on_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize) {
        self.emitted = true;
        self.inner.on_tool_use_start(name, provider_id, index);
    }
    fn on_error(&mut self, error: &str) {
        self.inner.on_error(error);
    }
    fn on_raw_line(&mut self, line: &str) {
        self.inner.on_raw_line(line);
    }
    fn on_block_complete(&mut self, full_text: &str) {
        self.inner.on_block_complete(full_text);
    }
    fn on_thinking(&mut self, text: &str) {
        self.emitted = true;
        self.inner.on_thinking(text);
    }
}

// ---------------------------------------------------------------------------
// Non-streaming fallback
// ---------------------------------------------------------------------------

/// Parameters needed to build and send an Anthropic API request.
/// Extracted so the non-streaming fallback can live in its own file without
/// needing a reference to `AnthropicProvider`.
pub(crate) struct RequestParams<'a> {
    pub model: String,
    pub max_tokens: u32,
    pub effort: Option<String>,
    pub base_url: String,
    pub headers: HeaderMap,
    pub http: &'a reqwest::Client,
}

/// Send a single non-streaming request and feed the result into `handler`.
///
/// `cancel` is required by `HttpAttemptExecutor` so that long body reads during
/// a fallback can be aborted cooperatively — the streaming path's `cancel` is
/// forwarded here to keep fallback cancellation latency aligned with the
/// streaming path.
pub(crate) async fn send_message_non_stream(
    params: RequestParams<'_>,
    system: &[SystemBlock],
    messages: &[Message],
    tool_schemas: &[serde_json::Value],
    handler: &mut dyn LegacyStreamSink,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut api_messages = convert_messages(messages);
    apply_message_cache_breakpoint(&mut api_messages);

    let model_label = params.model.clone();
    let request = CreateMessageRequest::new(
        params.model,
        params.max_tokens,
        params.effort,
        system.to_vec(),
        api_messages,
        sanitize_tool_schemas(tool_schemas),
        false,
    );

    let url = format!("{}/v1/messages", params.base_url);
    let request_json = request.clone().into_json();
    let request_bytes = serde_json::to_string(&request_json)
        .map(|value| value.len())
        .unwrap_or(0);

    let response = match HttpAttemptExecutor::execute(
        params
            .http
            .post(&url)
            .headers(params.headers)
            .json(&request_json),
        &HttpAttemptContext {
            driver: "anthropic",
            api: "messages_non_stream",
            provider: "anthropic",
            model: &model_label,
            method: "POST",
            endpoint: &url,
            attempt: 1,
            max_attempts: 1,
            message_count: messages.len(),
            tool_count: tool_schemas.len(),
            request_bytes,
        },
        cancel,
    )
    .await
    {
        Ok(success) => success.response,
        Err(failure) => {
            // 单次记录：非流式路径只有一次尝试，失败即终态，
            // 消费式 log(FinalFailure) 保证 error! 级别输出且只记一次。
            failure.log(AttemptDisposition::FinalFailure);
            return Err(match failure {
                HttpAttemptFailure::Cancelled => crate::LlmError::Cancelled,
                HttpAttemptFailure::Network { source, .. } => {
                    let mut msg = format!("{}\n  URL: {}", source, url);
                    let mut cause: Option<&dyn std::error::Error> =
                        std::error::Error::source(&source);
                    let mut depth = 1;
                    while let Some(c) = cause {
                        msg.push_str(&format!("\n  Cause #{}: {}", depth, c));
                        cause = c.source();
                        depth += 1;
                    }
                    crate::LlmError::Network(msg)
                }
                HttpAttemptFailure::Http {
                    status, kind, body, ..
                } => match kind {
                    HttpFailureKind::RateLimited => crate::LlmError::RateLimited,
                    HttpFailureKind::ContextTooLong => crate::LlmError::ContextTooLong,
                    HttpFailureKind::Server | HttpFailureKind::Client => crate::LlmError::Api {
                        error_type: status.to_string(),
                        message: body.text().to_string(),
                    },
                },
            });
        }
    };

    let body: serde_json::Value = match HttpAttemptExecutor::read_success_json(response, cancel)
        .await
    {
        Ok(body) => body,
        Err(SuccessBodyReadError::Cancelled) => return Err(crate::LlmError::Cancelled),
        Err(SuccessBodyReadError::Decode(e)) => return Err(crate::LlmError::Stream(e.to_string())),
    };

    // Parse the non-streaming response into StreamResponse
    let mut content_blocks = Vec::new();
    if let Some(content) = body.get("content").and_then(|v| v.as_array()) {
        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        handler.on_text(text);
                        handler.on_block_complete(text);
                        content_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| {
                            // 非流式响应已通过 HTTP 完整接收，input 缺失属 provider 协议异常。
                            // 记 warn 让 silent 变 visible；空对象作为兜底避免整个响应失败。
                            // 真正的"截断"问题在流式路径处理（见 business/stream.rs）。
                            log::warn!(
                                target: "aemeath:agent:provider",
                                "Anthropic 非流式响应 tool_use 块缺少 input 字段（id={}, name={}），使用空对象兜底",
                                id, name,
                            );
                            serde_json::Value::Object(serde_json::Map::new())
                        });
                    let idx = content_blocks.len();
                    handler.on_tool_use_start(&name, Some(&id), idx);
                    content_blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                _ => {}
            }
        }
    }

    let usage = usage_from_anthropic_response(&body);

    let stop_reason_str = body
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
            metadata: None,
        },
        usage,
        stop_reason: StopReason::parse(stop_reason_str),
    })
}

fn usage_from_anthropic_response(body: &serde_json::Value) -> Usage {
    let mut usage = Usage {
        input_tokens: body
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_tokens: body
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        cached_tokens: body
            .get("usage")
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        cache_creation_tokens: body
            .get("usage")
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        reasoning_tokens: None, // Anthropic 不返回 reasoning_tokens
        total_tokens: None,
    };
    usage.finalize_anthropic_total_tokens();
    usage
}

#[cfg(test)]
mod tests {
    use super::{
        apply_message_cache_breakpoint, convert_messages, sanitize_tool_schemas,
        send_message_non_stream, usage_from_anthropic_response, RequestParams,
    };
    use crate::ports::LegacyStreamSink;
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
    use share::message::{
        ContentBlock, ImageSource, Message, MessageMetadata, MessageSource, Role,
    };
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    /// Minimal LegacyStreamSink for tests — records calls without asserting layout.
    struct RecordingHandler;

    impl LegacyStreamSink for RecordingHandler {
        fn on_text(&mut self, _text: &str) {}
        fn on_tool_use_start(&mut self, _name: &str, _provider_id: Option<&str>, _index: usize) {}
        fn on_error(&mut self, _error: &str) {}
    }

    /// Spawn a single-shot HTTP server that writes `raw_response` and closes.
    async fn spawn_test_server(raw_response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0_u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(raw_response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{addr}")
    }

    fn dummy_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    #[tokio::test]
    async fn send_message_non_stream_maps_http_error_to_llm_error_api() {
        // 500 response with a small JSON body — the executor returns a bounded
        // body which the driver forwards verbatim into LlmError::Api.message.
        let url = spawn_test_server(
            "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 16\r\n\r\n{\"error\":\"boom\"}",
        )
        .await;

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url.trim_end_matches("/v1/messages").to_string(),
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = RecordingHandler;

        let err = send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected a 500 → LlmError::Api");

        match err {
            crate::LlmError::Api {
                error_type,
                message,
            } => {
                assert_eq!(error_type, "500 Internal Server Error");
                assert!(
                    message.contains("boom"),
                    "expected body to be forwarded, got {message}"
                );
            }
            other => panic!("expected LlmError::Api, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_message_non_stream_maps_client_error_to_llm_error_api() {
        // 400 — non-retryable, non-413 client error → LlmError::Api.
        let url = spawn_test_server(
            "HTTP/1.1 400 Bad Request\r\ncontent-length: 10\r\n\r\n{\"e\":\"bad\"}",
        )
        .await;

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url.trim_end_matches("/v1/messages").to_string(),
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = RecordingHandler;

        let err = send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected a 400 → LlmError::Api");

        assert!(
            matches!(err, crate::LlmError::Api { ref error_type, .. } if error_type == "400 Bad Request"),
            "expected Api(400), got {err:?}"
        );
    }

    #[tokio::test]
    async fn send_message_non_stream_maps_success_response() {
        // Minimal valid Anthropic non-stream response shape.
        let body = r#"{"content":[{"type":"text","text":"hi"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":2}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let url = spawn_test_server_static(&response).await;

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url.trim_end_matches("/v1/messages").to_string(),
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = RecordingHandler;

        let response = send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect("expected 200 → Ok(StreamResponse)");

        assert_eq!(
            response.stop_reason,
            crate::domain::invoke::StopReason::EndTurn
        );
        assert_eq!(response.usage.input_tokens, 3);
        assert_eq!(response.usage.output_tokens, 2);
    }

    async fn spawn_test_server_static(raw_response: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let raw_response = raw_response.to_owned();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0_u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(raw_response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn send_message_non_stream_maps_cancellation_to_llm_error_cancelled() {
        // The TcpListener below never accepts; combined with a pre-cancelled
        // token, the executor returns Cancelled before any send completes.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Drop the listener so connect() fails fast with ConnectionRefused;
        // the cancelled token guarantees Cancelled, not Network.
        drop(listener);
        let url = format!("http://{addr}");

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url,
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let messages = vec![Message::user("hi")];
        let mut handler = RecordingHandler;

        let err = send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected cancelled");

        assert!(
            matches!(err, crate::LlmError::Cancelled),
            "expected Cancelled, got {err:?}"
        );
    }

    /// Minimal LegacyStreamSink that records whether *any* output method fired —
    /// used to assert that a cancelled attempt produces no user-visible
    /// output at all.
    #[derive(Default)]
    struct CallTrackingHandler {
        called: bool,
    }

    impl LegacyStreamSink for CallTrackingHandler {
        fn on_text(&mut self, _text: &str) {
            self.called = true;
        }
        fn on_tool_use_start(&mut self, _name: &str, _provider_id: Option<&str>, _index: usize) {
            self.called = true;
        }
        fn on_error(&mut self, _error: &str) {
            self.called = true;
        }
        fn on_raw_line(&mut self, _line: &str) {
            self.called = true;
        }
        fn on_block_complete(&mut self, _full_text: &str) {
            self.called = true;
        }
        fn on_thinking(&mut self, _text: &str) {
            self.called = true;
        }
    }

    /// Review finding #2: a *successful* (200) response whose body stream
    /// stalls after headers arrive must still be cancellable. Cancellation
    /// during the body read must return `LlmError::Cancelled` promptly and
    /// must not emit any output through `handler`.
    ///
    /// `HttpAttemptExecutor::execute` only guards the pre-body network round
    /// trip (`request.send()`); the `response.json().await` call below it
    /// runs with no `cancel` awareness at all, so a stalled body currently
    /// blocks forever instead of surfacing `LlmError::Cancelled`. This test
    /// wraps the call in a bounded `tokio::time::timeout` so the failure
    /// mode is a clear assertion/panic rather than a hung test process.
    #[tokio::test]
    async fn send_message_non_stream_cancellation_during_blocked_body_read_returns_cancelled_with_no_output(
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0_u8; 4096];
                let _ = socket.read(&mut buf).await;
                let head =
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 200\r\n\r\n";
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(b"{\"content\":[").await;
                // Advertise more bytes than are ever sent and never close
                // the socket, so the body read blocks indefinitely absent
                // cooperative cancellation.
                std::future::pending::<()>().await;
            }
        });
        let url = format!("http://{addr}");

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url,
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        let cancel_trigger = cancel.clone();
        tokio::spawn(async move {
            // Give the header round-trip time to land before cancelling
            // mid-body.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            cancel_trigger.cancel();
        });
        let messages = vec![Message::user("hi")];
        let mut handler = CallTrackingHandler::default();

        let outcome = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel),
        )
        .await;

        let result = outcome.unwrap_or_else(|_| {
            panic!(
                "cancellation during a blocked non-stream body read must return promptly; \
                 `response.json().await` does not observe `cancel` at all, so the call was \
                 still hanging 500ms after the token was cancelled"
            )
        });

        assert!(
            matches!(result, Err(crate::LlmError::Cancelled)),
            "expected Cancelled, got {result:?}"
        );
        assert!(
            !handler.called,
            "handler must receive no output when the attempt is cancelled mid-body"
        );
    }

    /// Review finding #3: the non-stream fallback must classify HTTP
    /// failures by `HttpFailureKind` just like the streaming retry loop
    /// does (`stream_message_retries_429_then_returns_rate_limited` in
    /// `anthropic.rs`), not flatten every non-2xx response into a generic
    /// `LlmError::Api`. `send_message_non_stream` currently matches only on
    /// `HttpAttemptFailure::Http { status, body, .. }` and ignores `kind`
    /// entirely.
    #[tokio::test]
    async fn send_message_non_stream_maps_429_to_rate_limited() {
        let body = "{\"error\":\"slow down\"}";
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let url = spawn_test_server_static(&response).await;

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url.trim_end_matches("/v1/messages").to_string(),
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = RecordingHandler;

        let err = send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected a 429 → LlmError::RateLimited");

        assert!(
            matches!(err, crate::LlmError::RateLimited),
            "expected RateLimited (per HttpFailureKind::RateLimited classification), got {err:?}"
        );
    }

    /// See `send_message_non_stream_maps_429_to_rate_limited` — same finding,
    /// for the 413/ContextTooLong classification.
    #[tokio::test]
    async fn send_message_non_stream_maps_413_to_context_too_long() {
        let body = "{\"error\":\"too big\"}";
        let response = format!(
            "HTTP/1.1 413 Payload Too Large\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let url = spawn_test_server_static(&response).await;

        let http = reqwest::Client::new();
        let params = RequestParams {
            model: "claude-test".to_string(),
            max_tokens: 64,
            effort: None,
            base_url: url.trim_end_matches("/v1/messages").to_string(),
            headers: dummy_headers(),
            http: &http,
        };
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = RecordingHandler;

        let err = send_message_non_stream(params, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected a 413 → LlmError::ContextTooLong");

        assert!(
            matches!(err, crate::LlmError::ContextTooLong),
            "expected ContextTooLong (per HttpFailureKind::ContextTooLong classification), got {err:?}"
        );
    }

    #[test]
    fn strips_data_schema_and_keeps_allowed_fields() {
        let schemas = vec![serde_json::json!({
            "name": "Read",
            "description": "Read a file",
            "input_schema": {"type": "object"},
            "data_schema": {"type": "object"},
            "cache_control": {"type": "ephemeral"}
        })];
        let result = sanitize_tool_schemas(&schemas);
        assert_eq!(result.len(), 1);
        let tool = &result[0];
        assert!(tool.get("name").is_some());
        assert!(tool.get("description").is_some());
        assert!(tool.get("input_schema").is_some());
        assert!(tool.get("cache_control").is_some());
        assert!(
            tool.get("data_schema").is_none(),
            "data_schema must be stripped"
        );
    }

    #[test]
    fn preserves_input_schema_content_intact() {
        let input = serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {"type": "string"}
            },
            "required": ["file_path"]
        });
        let schemas = vec![serde_json::json!({
            "name": "Read",
            "description": "Read",
            "input_schema": input.clone(),
            "data_schema": {"type": "object"},
        })];
        let result = sanitize_tool_schemas(&schemas);
        assert_eq!(result[0].get("input_schema").unwrap(), &input);
    }

    #[test]
    fn non_stream_usage_includes_anthropic_cache_read_and_creation_fields() {
        let body = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 80,
                "cache_creation_input_tokens": 30,
                "output_tokens": 20
            }
        });

        let usage = usage_from_anthropic_response(&body);

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.cached_tokens, Some(80));
        assert_eq!(usage.cache_creation_tokens, Some(30));
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, Some(230));
    }

    #[test]
    fn handles_empty_schemas() {
        let result = sanitize_tool_schemas(&[]);
        assert!(result.is_empty());
    }

    // --- convert_messages tests ---

    #[test]
    fn convert_messages_strips_metadata() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "hi".to_string(),
            }],
            metadata: Some(MessageMetadata {
                source: MessageSource::SystemGenerated,
            }),
        };
        let result = convert_messages(&[msg]);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].get("metadata").is_none(),
            "metadata must be stripped"
        );
        assert_eq!(result[0]["role"], "user");
    }

    #[test]
    fn convert_messages_text_block() {
        let msg = Message::user("hello world");
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "text");
        assert_eq!(block["text"], "hello world");
    }

    #[test]
    fn convert_messages_image_strips_placeholder() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".to_string(),
                    data: "abc123".to_string(),
                },
                placeholder: Some("[Image #1]".to_string()),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert_eq!(block["source"]["data"], "abc123");
        assert!(
            block.get("placeholder").is_none(),
            "placeholder must be stripped"
        );
    }

    #[test]
    fn convert_messages_tool_use() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/a"}),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "tu_1");
        assert_eq!(block["name"], "Read");
        assert_eq!(block["input"]["file_path"], "/tmp/a");
    }

    #[test]
    fn convert_messages_tool_result_strips_text_field() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: serde_json::json!("done"),
                is_error: false,
                text: Some("done".to_string()),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "tu_1");
        assert_eq!(block["content"], "done");
        assert_eq!(block["is_error"], false);
        assert!(block.get("text").is_none(), "text field must be stripped");
    }

    #[test]
    fn convert_messages_tool_result_with_structured_content_uses_text_first() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: serde_json::json!({"stdout": "structured output"}),
                is_error: false,
                text: Some("plain output".to_string()),
            }],
            metadata: None,
        };

        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];

        assert_eq!(block["content"], "plain output");
        assert!(block["content"].is_string());
    }

    #[test]
    fn convert_messages_legacy_object_tool_result_serializes_content() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: serde_json::json!({"stdout": "legacy output"}),
                is_error: false,
                text: None,
            }],
            metadata: None,
        };

        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];

        assert_eq!(block["content"], r#"{"stdout":"legacy output"}"#);
        assert!(block["content"].is_string());
    }

    #[test]
    fn convert_messages_thinking_block_without_signature_is_stripped() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "let me think".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "answer".to_string(),
                },
            ],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let content = result[0]["content"].as_array().unwrap();
        // 无 signature 的 thinking block 被剥离，只保留 text
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn convert_messages_thinking_block_with_signature_preserved() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Thinking {
                thinking: "let me think".to_string(),
                signature: Some("sig_abc".to_string()),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "thinking");
        assert_eq!(block["thinking"], "let me think");
        assert_eq!(block["signature"], "sig_abc");
    }

    #[test]
    fn convert_messages_assistant_role() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "ok".to_string(),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        assert_eq!(result[0]["role"], "assistant");
    }

    // --- apply_message_cache_breakpoint tests ---

    #[test]
    fn cache_breakpoint_injected_on_penultimate_message() {
        let messages = vec![
            Message::user("first"),
            Message::user("second"),
            Message::user("third"),
        ];
        let mut api = convert_messages(&messages);
        apply_message_cache_breakpoint(&mut api);

        // penultimate = index 1
        let penultimate_content = api[1]["content"].as_array().unwrap();
        let last_block = penultimate_content.last().unwrap();
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");

        // last message (index 2) should NOT have cache_control
        let last_content = api[2]["content"].as_array().unwrap();
        assert!(
            last_content.last().unwrap().get("cache_control").is_none(),
            "last message must not have cache_control"
        );
    }

    #[test]
    fn cache_breakpoint_single_message_noop() {
        let messages = vec![Message::user("only")];
        let mut api = convert_messages(&messages);
        apply_message_cache_breakpoint(&mut api);

        let content = api[0]["content"].as_array().unwrap();
        assert!(
            content.last().unwrap().get("cache_control").is_none(),
            "single message should not get cache_control"
        );
    }

    #[test]
    fn cache_breakpoint_empty_messages_noop() {
        let mut api: Vec<serde_json::Value> = vec![];
        apply_message_cache_breakpoint(&mut api);
        assert!(api.is_empty());
    }

    #[test]
    fn cache_breakpoint_two_messages_hits_first() {
        let messages = vec![Message::user("a"), Message::user("b")];
        let mut api = convert_messages(&messages);
        apply_message_cache_breakpoint(&mut api);

        // penultimate = index 0
        let content = api[0]["content"].as_array().unwrap();
        assert_eq!(
            content.last().unwrap()["cache_control"]["type"],
            "ephemeral"
        );
    }
}
