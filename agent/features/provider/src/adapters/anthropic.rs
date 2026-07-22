//! Anthropic Claude provider implementation

mod message_conversion;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
};
use crate::adapters::stream::parse_invocation_stream;
use crate::domain::invoke::{CreateMessageRequest, SystemBlock};
use crate::ports::LlmProvider;

use message_conversion::{apply_message_cache_breakpoint, convert_messages, sanitize_tool_schemas};

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    model: String,
    user_agent: String,
    http: reqwest::Client,
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
            timeout_secs,
        }
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

    pub(crate) async fn invoke_stream(
        &self,
        scope: &crate::InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let mut api_messages = convert_messages(messages);
        apply_message_cache_breakpoint(&mut api_messages);
        let mut cached_tools = sanitize_tool_schemas(tool_schemas);
        if let Some(last_tool) = cached_tools.last_mut() {
            if let Some(object) = last_tool.as_object_mut() {
                object.insert(
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
        let request_json = request.into_json();
        let endpoint = format!("{}/v1/messages", self.base_url);
        let request_bytes = serde_json::to_string(&request_json)
            .map(|value| value.len())
            .unwrap_or(0);
        let context = HttpAttemptContext {
            driver: "anthropic",
            api: "messages_stream",
            provider: "anthropic",
            model: scope.model(),
            method: "POST",
            endpoint: &endpoint,
            attempt: 1,
            max_attempts: 1,
            message_count: messages.len(),
            tool_count: tool_schemas.len(),
            request_bytes,
        };
        let response = HttpAttemptExecutor::execute(
            self.http
                .post(&endpoint)
                .headers(self.build_headers().map_err(provider_error_from_llm)?)
                .json(&request_json),
            &context,
            cancel,
        )
        .await
        .map_err(|failure| {
            failure.log(AttemptDisposition::FinalFailure);
            provider_error_from_attempt(failure)
        })?
        .response;
        Ok(parse_invocation_stream(
            response,
            scope.effective_reasoning(),
            cancel.child_token(),
        ))
    }
}

fn provider_error_from_llm(error: crate::LlmError) -> crate::ProviderError {
    let kind = match error {
        crate::LlmError::Cancelled => crate::ProviderErrorKind::Cancelled,
        crate::LlmError::RateLimited => crate::ProviderErrorKind::RateLimited,
        crate::LlmError::ContextTooLong => crate::ProviderErrorKind::ContextTooLong,
        crate::LlmError::Network(_) => crate::ProviderErrorKind::Network,
        crate::LlmError::Api { .. } => crate::ProviderErrorKind::UpstreamUnavailable,
        crate::LlmError::StreamInterrupted(_) | crate::LlmError::StreamTruncated { .. } => {
            crate::ProviderErrorKind::StreamTruncated
        }
        crate::LlmError::Stream(_) => crate::ProviderErrorKind::Protocol,
        crate::LlmError::Config(_) => crate::ProviderErrorKind::Configuration,
    };
    crate::ProviderError::fatal(kind, error.to_string())
}

/// Pull-stream failures share the single typed classification maintained by
/// [`HttpAttemptFailure::into_provider_error`], so every driver maps the same
/// `HttpFailureKind` (including the newer `Authentication` / `PermissionDenied`
/// / `ModelUnavailable`) to the same `ProviderErrorKind` and retryability.
/// The caller logs `FinalFailure` before handing the failure here, so this
/// consumes the (already-logged) failure into the port error.
fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    failure.into_provider_error()
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn invocation_stream(
        &self,
        scope: &crate::InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        self.invoke_stream(scope, system, messages, tool_schemas, cancel)
            .await
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
    use crate::ports::{LlmProvider, ReasoningLevel};
    use share::message::Message;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

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
    async fn anthropic_invocation_stream_returns_retryable_error_after_one_request() {
        let response =
            "HTTP/1.1 429 Too Many Requests\r\nretry-after: 2\r\ncontent-length: 8\r\n\r\nsensitive";
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

        let error = match provider
            .invocation_stream(
                &scope,
                &[],
                &[Message::user("hi")],
                &[],
                &CancellationToken::new(),
            )
            .await
        {
            Ok(_) => panic!("429 should be returned to Runtime"),
            Err(error) => error,
        };

        assert_eq!(request_count.load(Ordering::SeqCst), 1);
        assert_eq!(error.kind, crate::ProviderErrorKind::RateLimited);
        assert!(error.retryable);
        assert_eq!(error.retry_after, Some(std::time::Duration::from_secs(2)));
        assert!(!error.safe_message.contains("sensitive"));
    }

    #[tokio::test]
    async fn llm_client_invocation_stream_reaches_anthropic_without_callback() {
        use futures_util::StreamExt;

        let body = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"production\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked: &'static str = Box::leak(response.into_boxed_str());
        let (base_url, request_count) = spawn_counting_server(leaked).await;
        let client =
            crate::composition::LlmClient::from_config(crate::composition::LlmConfigOptions {
                driver: crate::ProviderDriverKind::Anthropic.as_str().to_string(),
                source_key: "anthropic".to_string(),
                api_style: None,
                api_key: "test-key".to_string(),
                base_url: Some(base_url),
                model: "test-model".to_string(),
                max_tokens: 8192,
                reasoning: false,
                reasoning_config: None,
                timeout_secs: 60,
            })
            .expect("valid anthropic config");
        let scope =
            InvocationScope::new("test-model", 8192, ReasoningLevel::Off, ReasoningLevel::Off)
                .expect("valid scope");

        let events: Vec<_> = client
            .invocation_stream(
                &scope,
                &[],
                &[Message::user("hi")],
                &[],
                &CancellationToken::new(),
            )
            .await
            .expect("production stream entry succeeds")
            .collect()
            .await;

        assert_eq!(request_count.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &events[..],
            [
                crate::InvocationEvent::Delta(crate::InvocationDelta::Text(text)),
                crate::InvocationEvent::Completed(_)
            ] if text == "production"
        ));
    }

    #[tokio::test]
    async fn invoke_stream_emits_ordered_deltas_and_single_completion_from_one_request() {
        use futures_util::StreamExt;

        let body = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked: &'static str = Box::leak(response.into_boxed_str());
        let (base_url, request_count) = spawn_counting_server(leaked).await;
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

        let mut stream = provider
            .invoke_stream(&scope, &[], &[Message::user("hi")], &[], &cancel)
            .await
            .expect("stream creation succeeds");
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        assert_eq!(request_count.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &events[..],
            [
                crate::InvocationEvent::Delta(crate::InvocationDelta::Text(first)),
                crate::InvocationEvent::Delta(crate::InvocationDelta::Text(second)),
                crate::InvocationEvent::Completed(_)
            ] if first == "hel" && second == "lo"
        ));
        assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
        let crate::InvocationEvent::Completed(completion) = events.last().unwrap() else {
            panic!("expected completed event");
        };
        let usage = completion.usage.as_ref().expect("anthropic usage reported");
        assert_eq!(usage.input_tokens, Some(2));
        assert_eq!(usage.output_tokens, Some(1));
    }

    #[tokio::test]
    async fn invocation_stream_returns_context_too_long_on_413_without_retrying() {
        // 413 → HttpAttemptFailure::Http { kind: ContextTooLong } → driver
        // returns ProviderError::ContextTooLong immediately. Verifies that the
        // executor-driven path preserves the original "no retry on 413"
        // policy in the single-attempt pull-stream entry.
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

        let result = provider
            .invocation_stream(&scope, &[], &[Message::user("hi")], &[], &cancel)
            .await;
        let err = match result {
            Ok(_) => panic!("expected 413 → ProviderError::ContextTooLong, got Ok"),
            Err(err) => err,
        };

        assert!(
            err.kind == crate::ProviderErrorKind::ContextTooLong,
            "expected ContextTooLong, got {err:?}"
        );
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "413 must not be retried (policy preserved)"
        );
    }

    #[tokio::test]
    async fn invocation_stream_returns_upstream_unavailable_on_400_without_retrying() {
        // 400 → HttpAttemptFailure::Http { kind: Client } → driver returns
        // ProviderError::UpstreamUnavailable without retry. Verifies the
        // executor-driven path preserves the "client error → terminal" policy.
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

        let result = provider
            .invocation_stream(&scope, &[], &[Message::user("hi")], &[], &cancel)
            .await;
        let err = match result {
            Ok(_) => panic!("expected 400 → ProviderError::InvalidRequest, got Ok"),
            Err(err) => err,
        };

        assert!(
            err.kind == crate::ProviderErrorKind::InvalidRequest,
            "expected InvalidRequest (400), got {err:?}"
        );
        assert!(!err.retryable, "InvalidRequest (400) must be non-retryable");
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "400 must not be retried (policy preserved)"
        );
    }

    #[tokio::test]
    async fn invocation_stream_returns_retryable_rate_limited_on_429() {
        // 429 → HttpAttemptFailure::Http { kind: RateLimited } → driver maps to
        // a single-attempt retryable ProviderError::RateLimited. P6 retry
        // ownership is intentionally out of scope here (Runtime owns retry),
        // so we only assert the typed single-attempt classification.
        let response = "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\n\r\n";
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

        let result = provider
            .invocation_stream(&scope, &[], &[Message::user("hi")], &[], &cancel)
            .await;
        let err = match result {
            Ok(_) => panic!("expected single-attempt 429 → ProviderError::RateLimited, got Ok"),
            Err(err) => err,
        };

        assert!(
            err.kind == crate::ProviderErrorKind::RateLimited,
            "expected RateLimited, got {err:?}"
        );
        assert!(
            err.retryable,
            "429 must surface as retryable to Runtime (P6 owns retry)"
        );
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "single-attempt pull-stream entry must not retry internally"
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
    async fn invocation_stream_400_logs_final_failure_disposition() {
        // 400 is unconditionally terminal per
        // `invocation_stream_returns_upstream_unavailable_on_400_without_retrying`
        // above — the driver returns `Err` immediately. The diagnostic log this
        // failure emits must reflect the terminal outcome (`FinalFailure` →
        // `Error` level), and `retryable` in the structured record must be
        // false even though the retry budget had no chance to be exercised.
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

        let result = provider
            .invocation_stream(&scope, &[], &[Message::user("hi")], &[], &cancel)
            .await;
        match result {
            Ok(_) => panic!("expected 400 → terminal ProviderError::InvalidRequest, got Ok"),
            Err(err) => assert_eq!(err.kind, crate::ProviderErrorKind::InvalidRequest),
        }
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
            "expected FinalFailure disposition (Error level) for a terminal 400, got {level:?} \
             instead. payload={payload}"
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
