//! Unified HTTP attempt execution for provider adapters.

use super::constants::{ERROR_BODY_LIMIT, REQUEST_ID_HEADERS};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, CONTENT_TYPE, RETRY_AFTER};

use super::error_log::{self, ErrorLogContext};

/// Single-attempt disposition — the adapter makes exactly one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptDisposition {
    /// No retry and no fallback: the error propagates to the caller.
    FinalFailure,
}

impl AttemptDisposition {
    pub(crate) fn retryable(self) -> bool {
        false
    }

    pub(crate) fn log_level(self) -> log::Level {
        log::Level::Error
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SafeResponseHeaders {
    content_type: Option<String>,
    provider_request_id: Option<String>,
    retry_after_ms: Option<u64>,
}

impl SafeResponseHeaders {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self::from_headers_at(headers, std::time::SystemTime::now())
    }

    pub(crate) fn from_headers_at(headers: &HeaderMap, now: std::time::SystemTime) -> Self {
        let content_type = header_text(headers, CONTENT_TYPE.as_str());
        let provider_request_id = REQUEST_ID_HEADERS
            .iter()
            .find_map(|name| header_text(headers, name));
        let retry_after_ms = header_text(headers, RETRY_AFTER.as_str())
            .and_then(|value| parse_retry_after(&value, now));
        Self {
            content_type,
            provider_request_id,
            retry_after_ms,
        }
    }

    pub(crate) fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub(crate) fn provider_request_id(&self) -> Option<&str> {
        self.provider_request_id.as_deref()
    }

    pub(crate) fn retry_after_ms(&self) -> Option<u64> {
        self.retry_after_ms
    }
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn parse_retry_after(value: &str, now: std::time::SystemTime) -> Option<u64> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return seconds.checked_mul(1_000);
    }
    let deadline = httpdate::parse_http_date(value).ok()?;
    let delay = deadline.duration_since(now).ok()?;
    u64::try_from(delay.as_millis()).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkFailureKind {
    Connect,
    Timeout,
    Redirect,
    Request,
    Body,
    Decode,
    Unknown,
}

impl NetworkFailureKind {
    pub(crate) fn classify(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connect
        } else if error.is_redirect() {
            Self::Redirect
        } else if error.is_request() {
            Self::Request
        } else if error.is_body() {
            Self::Body
        } else if error.is_decode() {
            Self::Decode
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HttpFailureKind {
    Authentication,
    PermissionDenied,
    RateLimited,
    ContextTooLong,
    ModelUnavailable,
    Server,
    Client,
}

pub(crate) fn classify_http_status(status: reqwest::StatusCode) -> HttpFailureKind {
    match status {
        reqwest::StatusCode::UNAUTHORIZED => HttpFailureKind::Authentication,
        reqwest::StatusCode::FORBIDDEN => HttpFailureKind::PermissionDenied,
        reqwest::StatusCode::TOO_MANY_REQUESTS => HttpFailureKind::RateLimited,
        reqwest::StatusCode::PAYLOAD_TOO_LARGE => HttpFailureKind::ContextTooLong,
        reqwest::StatusCode::NOT_FOUND => HttpFailureKind::ModelUnavailable,
        status if status.is_server_error() => HttpFailureKind::Server,
        _ => HttpFailureKind::Client,
    }
}

/// 用错误响应 body 细化纯状态码分类（#1484）。
///
/// OpenAI 兼容网关（Wanaka、OpenAI 等）对上下文超限返回 **HTTP 400 + body**
/// `{"error":{"code":"context_length_exceeded",...}}`，仅靠状态码会被归为
/// `Client → InvalidRequest`（fatal），永远无法触发 runtime 的 auto compact。
/// 此函数在 body 读取完成后调用：只有基础分类为 `Client` 时才尝试提升，
/// 其余分类（413/401/403/429/404/5xx）原样短路，避免误伤既有语义。
pub(crate) fn refine_http_failure_kind(kind: HttpFailureKind, body_text: &str) -> HttpFailureKind {
    if kind != HttpFailureKind::Client {
        return kind;
    }
    if error_body_indicates_context_exceeded(body_text) {
        return HttpFailureKind::ContextTooLong;
    }
    kind
}

/// 解析 OpenAI 兼容错误 body，识别上下文超限特征。
///
/// 优先匹配结构化 `error.code == "context_length_exceeded"`（OpenAI 标准）；
/// 无 code 时兜底匹配 `error.message` 中的上下文窗口特征文案
/// （如 Wanaka 的 "Input exceeds the context window for ..."）。
fn error_body_indicates_context_exceeded(body_text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body_text) else {
        return false;
    };
    if let Some(code) = value
        .pointer("/error/code")
        .and_then(serde_json::Value::as_str)
    {
        return code == "context_length_exceeded";
    }
    value
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|message| {
            message.contains("context window")
                || message.contains("context length")
                || message.contains("maximum context length")
        })
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HttpAttemptContext<'a> {
    pub driver: &'a str,
    pub api: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub method: &'a str,
    pub endpoint: &'a str,
    pub attempt: u32,
    pub max_attempts: u32,
    pub message_count: usize,
    pub tool_count: usize,
    pub request_bytes: usize,
}

impl<'a> HttpAttemptContext<'a> {}

/// Captures every safe (non-secret) field the `error_log` module needs to
/// emit an `llm_api_error` diagnostic record, so a migrated driver never has
/// to reassemble an [`ErrorLogContext`] by hand from scattered local
/// variables. Prefer [`HttpAttemptFailure::log`] over reading these fields
/// directly.
///
/// Deliberately does *not* carry an [`AttemptDisposition`]: `execute` has no
/// way to know whether a given HTTP failure will be retried, is a terminal
/// failure, or triggers a fallback until the driver has classified it by
/// [`HttpFailureKind`] (or [`NetworkFailureKind`]) *after* the attempt
/// completes. Baking a pre-guessed disposition in here would let it drift
/// from the driver's actual, post-classification control-flow decision —
/// see [`HttpAttemptFailure::log`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticReceipt {
    driver: String,
    api: String,
    provider: String,
    model: String,
    method: String,
    endpoint: String,
    attempt: u32,
    max_attempts: u32,
    message_count: usize,
    tool_count: usize,
    request_bytes: usize,
    elapsed_ms: u128,
}

impl DiagnosticReceipt {
    fn capture(context: &HttpAttemptContext<'_>, elapsed: std::time::Duration) -> Self {
        Self {
            driver: context.driver.to_owned(),
            api: context.api.to_owned(),
            provider: context.provider.to_owned(),
            model: context.model.to_owned(),
            method: context.method.to_owned(),
            endpoint: context.endpoint.to_owned(),
            attempt: context.attempt,
            max_attempts: context.max_attempts,
            message_count: context.message_count,
            tool_count: context.tool_count,
            request_bytes: context.request_bytes,
            elapsed_ms: elapsed.as_millis(),
        }
    }

    fn error_log_context(&self) -> ErrorLogContext<'_> {
        ErrorLogContext {
            driver: &self.driver,
            api: &self.api,
            provider: &self.provider,
            model: &self.model,
            method: &self.method,
            endpoint: &self.endpoint,
            attempt: self.attempt,
            max_attempts: self.max_attempts,
            elapsed_ms: self.elapsed_ms,
            message_count: self.message_count,
            tool_count: self.tool_count,
            request_bytes: self.request_bytes,
        }
    }
}

#[derive(Debug)]
pub(crate) struct HttpAttemptSuccess {
    pub response: reqwest::Response,
}

#[derive(Debug)]
pub(crate) enum HttpAttemptFailure {
    Cancelled,
    Network {
        source: reqwest::Error,
        kind: NetworkFailureKind,
        // clippy(result_large_err)：DiagnosticReceipt 内联 6 个 String（约 152B），
        // 是 Network/Http 两个 variant 超过 200B 阈值的主因；装箱后签名与消费方不变。
        receipt: Box<DiagnosticReceipt>,
    },
    Http {
        status: reqwest::StatusCode,
        kind: HttpFailureKind,
        headers: SafeResponseHeaders,
        body: Box<BoundedErrorBody>,
        receipt: Box<DiagnosticReceipt>,
    },
}

impl HttpAttemptFailure {
    pub(crate) fn into_provider_error(self) -> crate::ProviderError {
        use crate::ProviderErrorKind;

        match self {
            Self::Cancelled => crate::ProviderError::cancelled(),
            Self::Network { kind, .. } => {
                let (error_kind, message) = match kind {
                    NetworkFailureKind::Timeout => {
                        (ProviderErrorKind::Timeout, "provider request timed out")
                    }
                    _ => (
                        ProviderErrorKind::Network,
                        "provider network request failed",
                    ),
                };
                crate::ProviderError::retryable(error_kind, message)
            }
            Self::Http {
                status,
                kind,
                headers,
                ..
            } => {
                let (error_kind, retryable, message) = match kind {
                    HttpFailureKind::Authentication => (
                        ProviderErrorKind::Authentication,
                        false,
                        "provider authentication failed",
                    ),
                    HttpFailureKind::PermissionDenied => (
                        ProviderErrorKind::PermissionDenied,
                        false,
                        "provider permission denied",
                    ),
                    HttpFailureKind::RateLimited => (
                        ProviderErrorKind::RateLimited,
                        false,
                        "provider rate limit exceeded",
                    ),
                    HttpFailureKind::ContextTooLong => (
                        ProviderErrorKind::ContextTooLong,
                        false,
                        "provider context limit exceeded",
                    ),
                    HttpFailureKind::ModelUnavailable => (
                        ProviderErrorKind::ModelUnavailable,
                        false,
                        "provider model unavailable",
                    ),
                    HttpFailureKind::Server => (
                        ProviderErrorKind::UpstreamUnavailable,
                        true,
                        "provider upstream unavailable",
                    ),
                    HttpFailureKind::Client => (
                        ProviderErrorKind::InvalidRequest,
                        false,
                        "provider rejected the request",
                    ),
                };
                let mut error = if retryable {
                    crate::ProviderError::retryable(error_kind, message)
                } else {
                    crate::ProviderError::fatal(error_kind, message)
                };
                error.provider_code = Some(status.as_u16().to_string());
                error.retry_after = headers
                    .retry_after_ms()
                    .map(std::time::Duration::from_millis);
                error
            }
        }
    }

    /// Emits the unified `llm_api_error` diagnostic record for this failure
    /// at the given `disposition`.
    ///
    /// This is the single, canonical logging call site a migrated driver
    /// should use in place of manually constructing an [`ErrorLogContext`]
    /// and invoking `error_log::log_network_error` / `log_http_error`
    /// directly. A cancelled attempt is deliberate (caller-initiated) and is
    /// intentionally not logged as an error — `disposition` is ignored for
    /// [`Self::Cancelled`]. `error_log::log_network_error` and
    /// `error_log::log_http_error` are HTTP-transport-only diagnostics; this
    /// is the sole call site for both.
    ///
    /// `disposition` is deliberately supplied by the caller rather than
    /// pre-guessed by `execute`: only the driver knows, after classifying
    /// this failure by [`HttpFailureKind`] / [`NetworkFailureKind`], whether
    /// it will retry, fall back, or give up — and that post-classification
    /// outcome is what must drive the log level, not a precompute made
    /// before the failure was even observed. Call this exactly once per
    /// failure, right after the disposition has been decided and before
    /// acting on it, so every failure is recorded exactly once at its real
    /// outcome.
    pub(crate) fn log(&self, disposition: AttemptDisposition) {
        match self {
            Self::Cancelled => {}
            Self::Network {
                source, receipt, ..
            } => {
                error_log::log_network_error(
                    receipt.error_log_context(),
                    source,
                    disposition.retryable(),
                    disposition.log_level(),
                );
            }
            Self::Http {
                status,
                headers,
                body,
                receipt,
                ..
            } => {
                error_log::log_http_error(
                    receipt.error_log_context(),
                    *status,
                    headers,
                    body,
                    disposition.retryable(),
                    disposition.log_level(),
                );
            }
        }
    }
}

pub(crate) struct HttpAttemptExecutor;

impl HttpAttemptExecutor {
    pub(crate) async fn execute(
        request: reqwest::RequestBuilder,
        context: &HttpAttemptContext<'_>,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<HttpAttemptSuccess, HttpAttemptFailure> {
        let started = std::time::Instant::now();
        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(HttpAttemptFailure::Cancelled),
            result = request.send() => result.map_err(|source| {
                let kind = NetworkFailureKind::classify(&source);
                let receipt = Box::new(DiagnosticReceipt::capture(context, started.elapsed()));
                HttpAttemptFailure::Network { source, kind, receipt }
            })?,
        };
        let status = response.status();
        if status.is_success() {
            return Ok(HttpAttemptSuccess { response });
        }

        let headers = SafeResponseHeaders::from_headers(response.headers());
        // 先读 body 再分类（#1484）：OpenAI 兼容网关用 400 + body error.code
        // 表达上下文超限，纯状态码分类无法识别，会漏掉 auto compact 自救链路。
        let body = Self::read_error_body(
            response,
            status,
            classify_http_status(status),
            &headers,
            context,
            started,
            cancel,
        )
        .await?;
        let kind = refine_http_failure_kind(classify_http_status(status), body.text());
        let receipt = Box::new(DiagnosticReceipt::capture(context, started.elapsed()));
        Err(HttpAttemptFailure::Http {
            status,
            kind,
            headers,
            body: Box::new(body),
            receipt,
        })
    }

    async fn read_error_body(
        response: reqwest::Response,
        status: reqwest::StatusCode,
        kind: HttpFailureKind,
        headers: &SafeResponseHeaders,
        context: &HttpAttemptContext<'_>,
        started: std::time::Instant,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<BoundedErrorBody, HttpAttemptFailure> {
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::with_capacity(ERROR_BODY_LIMIT);
        let mut observed = 0usize;
        let mut truncated = false;
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(HttpAttemptFailure::Cancelled),
                next = stream.next() => next,
            };
            let Some(chunk) = next else { break };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(source) => {
                    // The status line and headers were already observed as
                    // non-2xx before this body read started; a subsequent
                    // stream-level error must not discard that known
                    // status/kind by reclassifying as a generic Network
                    // failure. Surface it as an `Http` failure whose body
                    // is marked partial/truncated and carries the read
                    // error as an optional diagnostic.
                    let mut body = BoundedErrorBody::from_bytes(&bytes, ERROR_BODY_LIMIT);
                    body.observed_bytes = observed;
                    body.truncated = true;
                    body.read_error = Some(source.to_string());
                    let receipt = Box::new(DiagnosticReceipt::capture(context, started.elapsed()));
                    return Err(HttpAttemptFailure::Http {
                        status,
                        kind,
                        headers: headers.clone(),
                        body: Box::new(body),
                        receipt,
                    });
                }
            };
            observed = observed.saturating_add(chunk.len());
            let remaining = ERROR_BODY_LIMIT.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]); // allow unsafe_text_op: Vec slice (bytes)
            if observed > ERROR_BODY_LIMIT {
                truncated = true;
                break;
            }
        }
        let mut body = BoundedErrorBody::from_bytes(&bytes, ERROR_BODY_LIMIT);
        body.observed_bytes = observed;
        body.truncated = truncated;
        Ok(body)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedErrorBody {
    text: String,
    observed_bytes: usize,
    truncated: bool,
    read_error: Option<String>,
}

impl BoundedErrorBody {
    pub(crate) fn from_bytes(bytes: &[u8], limit: usize) -> Self {
        let end = bytes.len().min(limit);
        let text = String::from_utf8_lossy(&bytes[..end]).into_owned(); // allow unsafe_text_op: Vec slice (bytes)
        Self {
            text: text.trim_end_matches('\u{fffd}').to_string(),
            observed_bytes: bytes.len(),
            truncated: bytes.len() > limit,
            read_error: None,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn observed_bytes(&self) -> usize {
        self.observed_bytes
    }

    pub(crate) fn truncated(&self) -> bool {
        self.truncated
    }

    /// Diagnostic detail for a body read that was interrupted mid-stream
    /// (see `HttpAttemptExecutor::read_error_body`) — `None` when the body
    /// was read to completion (whether or not it hit the size bound).
    pub(crate) fn read_error(&self) -> Option<&str> {
        self.read_error.as_deref()
    }
}

#[cfg(test)]
#[path = "http_attempt_tests.rs"]
mod tests;
