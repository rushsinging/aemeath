//! 非流式请求：发送消息并等待完整响应

use super::{usage::parse_chat_usage, OpenAICompatibleProvider};
use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
    HttpFailureKind, SuccessBodyReadError,
};
use crate::domain::invoke::{InvocationScope, StreamResponse, SystemBlock};
use crate::ports::{ReasoningLevel, StreamHandler};
use share::message::{ContentBlock, Message, Role};
use tokio_util::sync::CancellationToken;

impl OpenAICompatibleProvider {
    pub(crate) async fn send_message_non_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let openai_messages = Self::convert_messages(
            system,
            messages,
            !matches!(scope.effective_reasoning(), ReasoningLevel::Off),
        )?;
        let tools = Self::convert_tools(tool_schemas);

        let mut request_body = self.base_request_body(scope, openai_messages, false);

        self.apply_reasoning_fields(&mut request_body, scope);
        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
            request_body["parallel_tool_calls"] = serde_json::Value::Bool(true);
        }

        let headers = self.build_headers()?;

        let endpoint = self.chat_url();
        let request_bytes = serde_json::to_string(&request_body)
            .map(|value| value.len())
            .unwrap_or(0);
        let context = HttpAttemptContext {
            driver: "openai_compatible",
            api: "chat_completions_non_stream",
            provider: &self.config.source_key,
            model: scope.model(),
            method: "POST",
            endpoint: &endpoint,
            attempt: 1,
            max_attempts: 1,
            message_count: messages.len(),
            tool_count: tool_schemas.len(),
            request_bytes,
        };
        let response = match HttpAttemptExecutor::execute(
            self.http
                .post(&endpoint)
                .headers(headers)
                .json(&request_body),
            &context,
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
                        let mut msg = format!("{}\n  URL: {}", source, endpoint);
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

        let body: serde_json::Value =
            match HttpAttemptExecutor::read_success_json(response, cancel).await {
                Ok(body) => body,
                Err(SuccessBodyReadError::Cancelled) => return Err(crate::LlmError::Cancelled),
                Err(SuccessBodyReadError::Decode(e)) => {
                    return Err(crate::LlmError::Stream(e.to_string()))
                }
            };

        // 解析响应
        let mut content_blocks = Vec::new();
        let mut stop_reason = crate::domain::invoke::StopReason::EndTurn;

        // 提取 usage
        let usage = body.get("usage").map(parse_chat_usage).unwrap_or_default();

        // 从 choices 中提取内容
        if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
            if let Some(choice) = choices.first() {
                // 检查 finish_reason
                if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = match finish {
                        "stop" => crate::domain::invoke::StopReason::EndTurn,
                        "tool_calls" => crate::domain::invoke::StopReason::ToolUse,
                        "length" => crate::domain::invoke::StopReason::MaxTokens,
                        _ => crate::domain::invoke::StopReason::EndTurn,
                    };
                }

                if let Some(message) = choice.get("message") {
                    // 提取 reasoning 内容（例如 glm-5.1, DeepSeek-R1）。
                    // 作为 Thinking 块保留在 content_blocks 中，以便下一轮
                    // convert_messages 可以将其作为 `reasoning_content` 字段重发——
                    // DeepSeek 的 thinking 模式拒绝省略此字段的 assistant 消息。
                    if let Some(reasoning) =
                        message.get("reasoning_content").and_then(|c| c.as_str())
                    {
                        if !reasoning.is_empty() {
                            handler.on_thinking(reasoning);
                            content_blocks.push(ContentBlock::Thinking {
                                thinking: reasoning.to_string(),
                                signature: None,
                            });
                        }
                    }

                    // 提取文本内容
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            handler.on_text(content);
                            handler.on_block_complete(content);
                            content_blocks.push(ContentBlock::Text {
                                text: content.to_string(),
                            });
                        }
                    }

                    // 提取 tool calls
                    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                        for (idx, tool_call) in tool_calls.iter().enumerate() {
                            if let Some(function) = tool_call.get("function") {
                                let id = tool_call
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let name = function
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let arguments = function
                                    .get("arguments")
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("{}");
                                let input: serde_json::Value = match serde_json::from_str(arguments)
                                {
                                    Ok(v) => v,
                                    Err(e) => {
                                        // 非流式 HTTP body 已完整接收，arguments 解析失败属 provider 协议异常。
                                        // 仍尝试启发式恢复（防 arguments 被切在 string 字面量中间），
                                        // 恢复失败则记 warn 用空对象兜底（避免整个响应失败）。
                                        log::warn!(
                                            target: "aemeath:agent:provider",
                                            "OpenAI 非流式 tool_call arguments 解析失败（id={}, name={}, err={}, raw_len={}），尝试恢复",
                                            id, name, e, arguments.len(),
                                        );
                                        crate::adapters::json_recovery::try_complete_truncated_json(arguments)
                                            .unwrap_or_else(|| {
                                                log::warn!(
                                                    target: "aemeath:agent:provider",
                                                    "启发式恢复未成功（id={}, name={}），使用空对象兜底",
                                                    id, name,
                                                );
                                                serde_json::Value::Object(serde_json::Map::new())
                                            })
                                    }
                                };

                                handler.on_tool_use_start(&name, Some(&id), idx);
                                if !name.is_empty() {
                                    content_blocks.push(ContentBlock::ToolUse { id, name, input });
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: content_blocks,
                metadata: None,
            },
            usage,
            stop_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::client::OpenAIProviderConfig;
    use crate::domain::invoke::InvocationScope;
    use tokio::net::TcpListener;

    /// Minimal StreamHandler that records whether *any* output method fired —
    /// used to assert that a cancelled attempt produces no user-visible
    /// output at all.
    #[derive(Default)]
    struct CallTrackingHandler {
        called: bool,
    }

    impl StreamHandler for CallTrackingHandler {
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

    fn test_provider(base_url: String) -> OpenAICompatibleProvider {
        let config = OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::OpenAI, "test");
        OpenAICompatibleProvider::new(
            config,
            "test-key".to_string(),
            Some(base_url),
            Some("test-model".to_string()),
            8192,
            false,
            None,
            60,
        )
    }

    fn test_scope() -> InvocationScope {
        InvocationScope::new(
            "test-model",
            8192,
            crate::ports::ReasoningLevel::Off,
            crate::ports::ReasoningLevel::Off,
        )
        .expect("valid scope")
    }

    async fn spawn_single_shot_server(raw_response: &str) -> String {
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

    /// Review finding #3: the non-stream fallback must classify HTTP
    /// failures by `HttpFailureKind` just like the streaming retry loop in
    /// `request_body.rs` does, not flatten every non-2xx response into a
    /// generic `LlmError::Api`. `send_message_non_stream` currently matches
    /// only on `HttpAttemptFailure::Http { status, body, .. }` and ignores
    /// `kind` entirely.
    #[tokio::test]
    async fn send_message_non_stream_maps_429_to_rate_limited() {
        let body = "{\"error\":\"slow down\"}";
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let base_url = spawn_single_shot_server(&response).await;
        let provider = test_provider(base_url);
        let scope = test_scope();
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = CallTrackingHandler::default();

        let err = provider
            .send_message_non_stream(&scope, &[], &messages, &[], &mut handler, &cancel)
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
        let base_url = spawn_single_shot_server(&response).await;
        let provider = test_provider(base_url);
        let scope = test_scope();
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = CallTrackingHandler::default();

        let err = provider
            .send_message_non_stream(&scope, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected a 413 → LlmError::ContextTooLong");

        assert!(
            matches!(err, crate::LlmError::ContextTooLong),
            "expected ContextTooLong (per HttpFailureKind::ContextTooLong classification), got {err:?}"
        );
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
                let _ = socket.write_all(b"{\"choices\":[").await;
                // Advertise more bytes than are ever sent and never close
                // the socket, so the body read blocks indefinitely absent
                // cooperative cancellation.
                std::future::pending::<()>().await;
            }
        });
        let base_url = format!("http://{addr}");
        let provider = test_provider(base_url);
        let scope = test_scope();

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
            provider.send_message_non_stream(&scope, &[], &messages, &[], &mut handler, &cancel),
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
}
