//! Anthropic Claude provider implementation

mod message_conversion;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
    HttpFailureKind, NetworkFailureKind,
};
use crate::adapters::stream::parse_stream;
use crate::domain::invoke::{CreateMessageRequest, StreamResponse, SystemBlock};
use crate::ports::{LlmProvider, StreamHandler};

use message_conversion::{
    apply_message_cache_breakpoint, convert_messages, sanitize_tool_schemas,
    send_message_non_stream, RequestParams, TrackingHandler,
};

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    model: String,
    user_agent: String,
    http: reqwest::Client,
    /// Maximum retry attempts (default 3)
    max_retries: u32,
    /// Request timeout in seconds — stored for diagnostics; the value is applied
    /// to the reqwest client at construction time.
    #[allow(dead_code)]
    timeout_secs: u64,
}

impl AnthropicProvider {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        _max_tokens: u32,
        _reasoning_level: crate::ports::ReasoningLevel,
        timeout_secs: u64,
    ) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
            user_agent: format!("aemeath/{}", share::version()),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(crate::CONNECT_TIMEOUT_SECS))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs,
        }
    }

    /// Set maximum retry attempts
    /// builder 方法当前无调用点，收窄可见性后暴露为孤儿，保留备用（refs #61 D3）。
    #[allow(dead_code)]
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set request timeout in seconds (builder 旋钮，当前无外部调用点).
    #[allow(dead_code)]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(crate::CONNECT_TIMEOUT_SECS))
            .build()
            .expect("failed to create HTTP client with custom timeout");
        self
    }

    fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key)
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static("prompt-caching-2024-07-31"),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );
        Ok(headers)
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream_message(
        &self,
        scope: &crate::InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let mut api_messages = convert_messages(messages);

        // 断点③：在 messages 倒数第二条消息上注入 cache_control，
        // 让 Anthropic 缓存整个对话历史前缀。配合断点①（system static）
        // 和断点②（tools），共使用 3/4 个允许的断点。
        apply_message_cache_breakpoint(&mut api_messages);

        // 先清洗 tool schema（移除 data_schema 等内部扩展字段），再为
        // 最后一个 tool 追加 cache_control 断点，让 Anthropic 缓存整个
        // tools schema（≈6K tokens）。后续 turn 命中 cache 后固定开销
        // 成本降至约 1/10。Anthropic 原生支持 tools 数组缓存。
        let mut cached_tools = sanitize_tool_schemas(tool_schemas);
        if let Some(last_tool) = cached_tools.last_mut() {
            if let Some(obj) = last_tool.as_object_mut() {
                obj.insert(
                    "cache_control".to_string(),
                    serde_json::json!({"type": "ephemeral"}),
                );
            }
        }
        let effort = match scope.effective_reasoning() {
            crate::ports::ReasoningLevel::Off => None,
            level => Some(level.as_str().to_string()),
        };
        let request = CreateMessageRequest::new(
            scope.model().to_string(),
            scope.max_tokens(),
            effort,
            system.to_vec(),
            api_messages,
            cached_tools,
            true,
        );

        let headers = self.build_headers()?;

        let request_json = request.clone().into_json();
        let request_bytes = serde_json::to_string(&request_json)
            .map(|value| value.len())
            .unwrap_or(0);
        let endpoint = format!("{}/v1/messages", self.base_url);
        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Cancelled);
            }

            if attempt > 0 {
                let delay =
                    std::time::Duration::from_millis((1000 * 2u64.pow(attempt)).min(30_000));
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        return Err(crate::LlmError::Cancelled);
                    }
                    _ = tokio::time::sleep(delay) => {}
                }
            }

            let context = HttpAttemptContext {
                driver: "anthropic",
                api: "messages_stream",
                provider: "anthropic",
                model: scope.model(),
                method: "POST",
                endpoint: &endpoint,
                attempt: attempt + 1,
                max_attempts: self.max_retries,
                message_count: messages.len(),
                tool_count: tool_schemas.len(),
                request_bytes,
            };

            let response = match HttpAttemptExecutor::execute(
                self.http
                    .post(&endpoint)
                    .headers(headers.clone())
                    .json(&request_json),
                &context,
                cancel,
            )
            .await
            {
                Ok(success) => success.response,
                Err(failure) => {
                    let remaining = self.max_retries.saturating_sub(attempt + 1);
                    // Disposition must reflect the driver's actual,
                    // post-classification control-flow decision below — not
                    // a pre-guess made before the failure kind was known.
                    // Client/ContextTooLong are unconditionally terminal
                    // regardless of remaining retry budget.
                    let disposition = match &failure {
                        HttpAttemptFailure::Cancelled => AttemptDisposition::FinalFailure,
                        HttpAttemptFailure::Network { .. } => {
                            AttemptDisposition::from_remaining(remaining)
                        }
                        HttpAttemptFailure::Http { kind, .. } => match kind {
                            HttpFailureKind::RateLimited | HttpFailureKind::Server => {
                                AttemptDisposition::from_remaining(remaining)
                            }
                            HttpFailureKind::ContextTooLong | HttpFailureKind::Client => {
                                AttemptDisposition::FinalFailure
                            }
                        },
                    };
                    // 单次记录：typed 分类决定 disposition 后，消费式
                    // failure.log(disposition) 只记一次，反映真实终态。
                    failure.log(disposition);
                    match failure {
                        HttpAttemptFailure::Cancelled => {
                            return Err(crate::LlmError::Cancelled);
                        }
                        HttpAttemptFailure::Network { source, kind, .. } => {
                            let detail = match kind {
                                NetworkFailureKind::Connect => "connection failed",
                                NetworkFailureKind::Timeout => "request timed out",
                                NetworkFailureKind::Redirect => "too many redirects",
                                NetworkFailureKind::Request => "request build error",
                                NetworkFailureKind::Body => "request body error",
                                NetworkFailureKind::Decode => "response decode error",
                                NetworkFailureKind::Unknown => "unknown",
                            };
                            if remaining > 0 {
                                handler.on_error(&format!(
                                    "network error ({detail}), retrying ({}/{})...",
                                    attempt + 2,
                                    self.max_retries
                                ));
                            }
                            let mut msg = format!("{} ({})\n  URL: {}", source, detail, endpoint);
                            let mut cause: Option<&dyn std::error::Error> =
                                std::error::Error::source(&source);
                            let mut depth = 1;
                            while let Some(c) = cause {
                                msg.push_str(&format!("\n  Cause #{}: {}", depth, c));
                                cause = c.source();
                                depth += 1;
                            }
                            last_error = Some(crate::LlmError::Network(msg));
                            continue;
                        }
                        HttpAttemptFailure::Http {
                            status, kind, body, ..
                        } => match kind {
                            HttpFailureKind::RateLimited => {
                                if remaining > 0 {
                                    handler.on_error(&format!(
                                        "rate limited ({}), retrying ({}/{})...",
                                        status,
                                        attempt + 2,
                                        self.max_retries
                                    ));
                                }
                                last_error = Some(crate::LlmError::RateLimited);
                                continue;
                            }
                            HttpFailureKind::ContextTooLong => {
                                return Err(crate::LlmError::ContextTooLong);
                            }
                            HttpFailureKind::Server => {
                                if remaining > 0 {
                                    handler.on_error(&format!(
                                        "server error ({}), retrying ({}/{})...",
                                        status,
                                        attempt + 2,
                                        self.max_retries
                                    ));
                                }
                                last_error = Some(crate::LlmError::Api {
                                    error_type: status.to_string(),
                                    message: body.text().to_string(),
                                });
                                continue;
                            }
                            HttpFailureKind::Client => {
                                return Err(crate::LlmError::Api {
                                    error_type: status.to_string(),
                                    message: body.text().to_string(),
                                });
                            }
                        },
                    }
                }
            };

            let mut tracking = TrackingHandler::new(handler);
            let stream_result = parse_stream(response, &mut tracking, cancel).await;
            let emitted = tracking.emitted;
            match stream_result {
                Ok(resp) => return Ok(resp),
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(msg)) => {
                    // Streaming failed for non-cancel reason.
                    // Only fall back to non-streaming if no partial output was emitted;
                    // otherwise retrying would duplicate already-rendered text in the UI.
                    if emitted {
                        return Err(crate::LlmError::Stream(format!(
                            "stream interrupted after partial output: {msg}"
                        )));
                    }
                    let effort = match scope.effective_reasoning() {
                        crate::ports::ReasoningLevel::Off => None,
                        level => Some(level.as_str().to_string()),
                    };
                    let params = RequestParams {
                        model: scope.model().to_string(),
                        max_tokens: scope.max_tokens(),
                        effort,
                        base_url: self.base_url.clone(),
                        headers: self.build_headers()?,
                        http: &self.http,
                    };
                    return send_message_non_stream(
                        params,
                        system,
                        messages,
                        tool_schemas,
                        handler,
                        cancel,
                    )
                    .await;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_error.unwrap_or(crate::LlmError::Network("max retries exceeded".to_string())))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn max_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        crate::ports::ReasoningLevel::Max
    }
}

#[cfg(test)]
mod tests {
    use super::AnthropicProvider;
    use crate::domain::invoke::{CreateMessageRequest, InvocationScope};
    use crate::ports::{LlmProvider, ReasoningLevel, StreamHandler};
    use share::message::Message;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    struct NoopHandler;

    impl StreamHandler for NoopHandler {
        fn on_text(&mut self, _text: &str) {}
        fn on_tool_use_start(&mut self, _name: &str, _provider_id: Option<&str>, _index: usize) {}
        fn on_error(&mut self, _error: &str) {}
    }

    /// Spawn a server that returns `raw_response` for every request and counts
    /// how many requests it has served.
    async fn spawn_counting_server(raw_response: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(p) => p,
                    Err(_) => break,
                };
                counter_clone.fetch_add(1, Ordering::SeqCst);
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0_u8; 8192];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(raw_response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}"), counter)
    }

    #[tokio::test]
    async fn stream_message_returns_context_too_long_on_413_without_retrying() {
        // 413 → HttpAttemptFailure::Http { kind: ContextTooLong } → driver
        // returns LlmError::ContextTooLong immediately. Verifies that the
        // executor-driven path preserves the original "no retry on 413"
        // policy.
        let response =
            "HTTP/1.1 413 Payload Too Large\r\ncontent-length: 17\r\n\r\n{\"err\":\"too big\"}";
        let (base_url, request_count) = spawn_counting_server(response).await;

        let provider = AnthropicProvider::new(
            "test-key".to_string(),
            Some(base_url),
            Some("test-model".to_string()),
            8192,
            ReasoningLevel::Off,
            60,
        );
        let scope =
            InvocationScope::new("test-model", 8192, ReasoningLevel::Off, ReasoningLevel::Off)
                .expect("valid scope");
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = NoopHandler;

        let err = provider
            .stream_message(&scope, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected 413 → LlmError::ContextTooLong");

        assert!(
            matches!(err, crate::LlmError::ContextTooLong),
            "expected ContextTooLong, got {err:?}"
        );
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "413 must not be retried (policy preserved)"
        );
    }

    #[tokio::test]
    async fn stream_message_returns_api_error_on_400_without_retrying() {
        // 400 → HttpAttemptFailure::Http { kind: Client } → driver returns
        // LlmError::Api without retry. Verifies the executor-driven path
        // preserves the "client error → terminal" policy.
        let response = "HTTP/1.1 400 Bad Request\r\ncontent-length: 10\r\n\r\n{\"e\":\"bad\"}";
        let (base_url, request_count) = spawn_counting_server(response).await;

        let provider = AnthropicProvider::new(
            "test-key".to_string(),
            Some(base_url),
            Some("test-model".to_string()),
            8192,
            ReasoningLevel::Off,
            60,
        );
        let scope =
            InvocationScope::new("test-model", 8192, ReasoningLevel::Off, ReasoningLevel::Off)
                .expect("valid scope");
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = NoopHandler;

        let err = provider
            .stream_message(&scope, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected 400 → LlmError::Api");

        assert!(
            matches!(err, crate::LlmError::Api { ref error_type, .. } if error_type == "400 Bad Request"),
            "expected Api(400), got {err:?}"
        );
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "400 must not be retried (policy preserved)"
        );
    }

    #[tokio::test]
    async fn stream_message_retries_429_then_returns_rate_limited() {
        // 429 → HttpAttemptFailure::Http { kind: RateLimited } → driver
        // continues the retry loop. After max_retries is exhausted, the loop
        // falls through to Err(last_error) which is RateLimited. The executor
        // path replaces the prior inline 429 branch.
        let response = "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\n\r\n";
        let (base_url, request_count) = spawn_counting_server(response).await;

        let provider = AnthropicProvider::new(
            "test-key".to_string(),
            Some(base_url),
            Some("test-model".to_string()),
            8192,
            ReasoningLevel::Off,
            60,
        )
        .with_max_retries(2);
        let scope =
            InvocationScope::new("test-model", 8192, ReasoningLevel::Off, ReasoningLevel::Off)
                .expect("valid scope");
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = NoopHandler;

        let err = provider
            .stream_message(&scope, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected retries exhausted → RateLimited");

        assert!(
            matches!(err, crate::LlmError::RateLimited),
            "expected RateLimited, got {err:?}"
        );
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            2,
            "429 should be retried up to max_retries (policy preserved)"
        );
    }

    // -----------------------------------------------------------------
    // Review finding #4: a terminal 400 (Client, non-retryable) hit while
    // the driver still has retry budget remaining must have its diagnostic
    // *disposition* reflect the actual outcome (`FinalFailure`), not the
    // driver's generic "remaining attempts > 0 → RetryPlanned" precompute.
    //
    // `DiagnosticReceipt::disposition` is private to `http_attempt.rs`, so
    // there is no way to read it back from outside that module. The only
    // externally observable signal of the chosen `AttemptDisposition` is
    // the log level `HttpAttemptFailure::log()` emits (`Error` for
    // `FinalFailure`, `Debug` for `RetryPlanned` — see
    // `AttemptDisposition::log_level`) and the `retryable` field baked into
    // the emitted `llm_api_error` JSON record. This thread-local capturing
    // logger is the "observable receipt/sink" needed to assert on that
    // signal without touching production code.
    // -----------------------------------------------------------------

    thread_local! {
        static CAPTURED_LLM_API_ERROR_LOGS: std::cell::RefCell<Vec<(log::Level, String)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    struct ThreadLocalCapturingLogger;

    impl log::Log for ThreadLocalCapturingLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if record.target() == crate::adapters::error_log::LLM_API_ERROR_TARGET {
                let level = record.level();
                let payload = format!("{}", record.args());
                CAPTURED_LLM_API_ERROR_LOGS.with(|cell| cell.borrow_mut().push((level, payload)));
            }
        }

        fn flush(&self) {}
    }

    /// Installs the capturing logger exactly once per test process. Safe to
    /// call from every test in this module: `log::set_logger` only succeeds
    /// once, subsequent calls are no-ops via `Once`. Capture storage itself
    /// is thread-local, so tests running on different OS threads (the
    /// default per-test-thread model used by `#[tokio::test]` with the
    /// default `current_thread` runtime flavor) never observe each other's
    /// captured records.
    fn install_capturing_logger() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_boxed_logger(Box::new(ThreadLocalCapturingLogger))
                .expect("capturing logger must install exactly once per process");
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    /// Drains (and clears) whatever `llm_api_error` records this thread has
    /// captured so far, guarding against leftover records from an earlier
    /// test sharing the same OS thread.
    fn drain_captured_logs() -> Vec<(log::Level, String)> {
        CAPTURED_LLM_API_ERROR_LOGS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
    }

    #[tokio::test]
    async fn stream_message_400_with_remaining_attempts_logs_final_failure_disposition() {
        // 400 is unconditionally terminal per
        // `stream_message_returns_api_error_on_400_without_retrying` above —
        // the driver returns `Err` immediately regardless of how much retry
        // budget remains. With the default `max_retries = 10`, attempt 0
        // still has 9 attempts of budget remaining, so the driver's
        // pre-classification `disposition` precompute
        // (`remaining > 0 → RetryPlanned`) picks `RetryPlanned` even though
        // the *actual* outcome for a Client-kind HTTP failure is always
        // `FinalFailure`. The diagnostic log this failure emits must reflect
        // the real (terminal) outcome, not the precompute.
        install_capturing_logger();
        drain_captured_logs();

        let response = "HTTP/1.1 400 Bad Request\r\ncontent-length: 10\r\n\r\n{\"e\":\"bad\"}";
        let (base_url, request_count) = spawn_counting_server(response).await;

        let provider = AnthropicProvider::new(
            "test-key".to_string(),
            Some(base_url),
            Some("test-model".to_string()),
            8192,
            ReasoningLevel::Off,
            60,
        );
        let scope =
            InvocationScope::new("test-model", 8192, ReasoningLevel::Off, ReasoningLevel::Off)
                .expect("valid scope");
        let cancel = CancellationToken::new();
        let messages = vec![Message::user("hi")];
        let mut handler = NoopHandler;

        let err = provider
            .stream_message(&scope, &[], &messages, &[], &mut handler, &cancel)
            .await
            .expect_err("expected 400 → terminal LlmError::Api");
        assert!(matches!(err, crate::LlmError::Api { .. }));
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "400 must not be retried even though retry budget remains"
        );

        let logs = drain_captured_logs();
        let llm_api_error_logs: Vec<_> = logs
            .iter()
            .filter(|(_, payload)| payload.contains("\"llm_api_error\""))
            .collect();
        assert_eq!(
            llm_api_error_logs.len(),
            1,
            "expected exactly one llm_api_error diagnostic record, got {llm_api_error_logs:?}"
        );
        let (level, payload) = llm_api_error_logs[0];

        assert_eq!(
            *level,
            log::Level::Error,
            "expected FinalFailure disposition (Error level) for a terminal 400 with retry \
             budget remaining, got {level:?} instead — the driver logs the pre-classification \
             disposition (RetryPlanned, because `remaining attempts > 0`) instead of the \
             post-classification terminal outcome (FinalFailure, because HttpFailureKind::Client \
             is unconditionally terminal). payload={payload}"
        );
        assert!(
            payload.contains("\"retryable\":false"),
            "expected retryable=false for a terminal 400, payload={payload}"
        );
    }

    #[test]
    fn anthropic_request_serializes_adaptive_thinking_with_effort() {
        let request = CreateMessageRequest::new(
            "claude-sonnet-4-6".to_string(),
            8192,
            Some("medium".to_string()),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
        );

        let value = request.into_json();
        assert_eq!(
            value.get("thinking").unwrap().get("type"),
            Some(&serde_json::json!("adaptive"))
        );
        assert_eq!(
            value.get("thinking").unwrap().get("display"),
            Some(&serde_json::json!("summarized"))
        );
        assert_eq!(
            value.get("output_config").unwrap().get("effort"),
            Some(&serde_json::json!("medium"))
        );
    }

    #[test]
    fn anthropic_request_off_thinking_disabled() {
        let request = CreateMessageRequest::new(
            "claude-sonnet-4-6".to_string(),
            8192,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
        );

        let value = request.into_json();
        assert_eq!(
            value.get("thinking").unwrap().get("type"),
            Some(&serde_json::json!("disabled"))
        );
        assert!(value.get("output_config").is_none());
    }
}
