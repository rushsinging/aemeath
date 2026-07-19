//! Unified HTTP attempt execution for provider adapters.

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, CONTENT_TYPE, RETRY_AFTER};

use super::error_log::{self, ErrorLogContext};

pub(crate) const ERROR_BODY_LIMIT: usize = 16 * 1024;

const REQUEST_ID_HEADERS: [&str; 4] = [
    "request-id",
    "x-request-id",
    "anthropic-request-id",
    "openai-request-id",
];

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
        receipt: DiagnosticReceipt,
    },
    Http {
        status: reqwest::StatusCode,
        kind: HttpFailureKind,
        headers: SafeResponseHeaders,
        body: BoundedErrorBody,
        receipt: DiagnosticReceipt,
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
                        true,
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
                let receipt = DiagnosticReceipt::capture(context, started.elapsed());
                HttpAttemptFailure::Network { source, kind, receipt }
            })?,
        };
        let status = response.status();
        if status.is_success() {
            return Ok(HttpAttemptSuccess { response });
        }

        let headers = SafeResponseHeaders::from_headers(response.headers());
        let kind = classify_http_status(status);
        let body =
            Self::read_error_body(response, status, kind, &headers, context, started, cancel)
                .await?;
        let receipt = DiagnosticReceipt::capture(context, started.elapsed());
        Err(HttpAttemptFailure::Http {
            status,
            kind,
            headers,
            body,
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
                    let receipt = DiagnosticReceipt::capture(context, started.elapsed());
                    return Err(HttpAttemptFailure::Http {
                        status,
                        kind,
                        headers: headers.clone(),
                        body,
                        receipt,
                    });
                }
            };
            observed = observed.saturating_add(chunk.len());
            let remaining = ERROR_BODY_LIMIT.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
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
        let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
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
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, RETRY_AFTER};

    #[test]
    fn safe_headers_extract_only_allowlisted_values() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-request-id", HeaderValue::from_static("request-123"));
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));

        let safe = SafeResponseHeaders::from_headers(&headers);

        assert_eq!(safe.content_type(), Some("application/json"));
        assert_eq!(safe.provider_request_id(), Some("request-123"));
        assert!(!format!("{safe:?}").contains("secret"));
    }

    #[test]
    fn retry_after_parses_seconds_http_date_and_rejects_invalid_values() {
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("17"));
        assert_eq!(
            SafeResponseHeaders::from_headers_at(&headers, now).retry_after_ms(),
            Some(17_000)
        );

        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Tue, 14 Nov 2023 22:13:25 GMT"),
        );
        assert_eq!(
            SafeResponseHeaders::from_headers_at(&headers, now).retry_after_ms(),
            Some(5_000)
        );

        headers.insert(RETRY_AFTER, HeaderValue::from_static("invalid"));
        assert_eq!(
            SafeResponseHeaders::from_headers_at(&headers, now).retry_after_ms(),
            None
        );
    }

    #[tokio::test]
    async fn network_failure_classifies_reqwest_error_categories() {
        let timeout = reqwest::Client::builder()
            .timeout(std::time::Duration::ZERO)
            .build()
            .unwrap();
        let error = timeout.get("http://127.0.0.1:9").send().await.unwrap_err();
        assert!(matches!(
            NetworkFailureKind::classify(&error),
            NetworkFailureKind::Timeout | NetworkFailureKind::Connect
        ));
    }

    #[test]
    fn http_status_classifies_stable_provider_error_categories() {
        let cases = [
            (
                reqwest::StatusCode::UNAUTHORIZED,
                HttpFailureKind::Authentication,
            ),
            (
                reqwest::StatusCode::FORBIDDEN,
                HttpFailureKind::PermissionDenied,
            ),
            (
                reqwest::StatusCode::PAYLOAD_TOO_LARGE,
                HttpFailureKind::ContextTooLong,
            ),
            (
                reqwest::StatusCode::TOO_MANY_REQUESTS,
                HttpFailureKind::RateLimited,
            ),
            (
                reqwest::StatusCode::NOT_FOUND,
                HttpFailureKind::ModelUnavailable,
            ),
            (reqwest::StatusCode::BAD_GATEWAY, HttpFailureKind::Server),
            (reqwest::StatusCode::BAD_REQUEST, HttpFailureKind::Client),
        ];

        for (status, expected) in cases {
            assert_eq!(classify_http_status(status), expected, "status={status}");
        }
    }

    #[tokio::test]
    async fn attempt_failure_maps_to_safe_provider_error_contract() {
        let cases = [
            (401, crate::ProviderErrorKind::Authentication, false, None),
            (403, crate::ProviderErrorKind::PermissionDenied, false, None),
            (413, crate::ProviderErrorKind::ContextTooLong, false, None),
            (
                429,
                crate::ProviderErrorKind::RateLimited,
                true,
                Some(3_000),
            ),
            (404, crate::ProviderErrorKind::ModelUnavailable, false, None),
            (
                502,
                crate::ProviderErrorKind::UpstreamUnavailable,
                true,
                None,
            ),
            (400, crate::ProviderErrorKind::InvalidRequest, false, None),
        ];

        for (status, expected_kind, expected_retryable, expected_retry_after_ms) in cases {
            let reason = reqwest::StatusCode::from_u16(status)
                .unwrap()
                .canonical_reason()
                .unwrap();
            let retry_after = if status == 429 {
                "retry-after: 3\r\n"
            } else {
                ""
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\n{retry_after}content-length: 21\r\n\r\nsecret-token-in-body"
            );
            let server = TestServer::start(&response).await;
            let url = server.url();
            let failure = HttpAttemptExecutor::execute(
                reqwest::Client::new().get(&url),
                &test_context(&url),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap_err();

            let error = failure.into_provider_error();
            assert_eq!(error.kind, expected_kind, "status={status}");
            assert_eq!(error.retryable, expected_retryable, "status={status}");
            assert_eq!(
                error.provider_code.as_deref(),
                Some(status.to_string().as_str())
            );
            assert_eq!(
                error
                    .retry_after
                    .map(|duration| duration.as_millis() as u64),
                expected_retry_after_ms,
                "status={status}"
            );
            assert!(!error.safe_message.contains("secret-token-in-body"));
        }
    }

    #[test]
    fn cancellation_maps_to_non_retryable_provider_error() {
        let error = HttpAttemptFailure::Cancelled.into_provider_error();
        assert_eq!(error.kind, crate::ProviderErrorKind::Cancelled);
        assert!(!error.retryable);
    }

    #[test]
    fn bounded_error_body_tracks_truncation_and_utf8_boundaries() {
        let body = BoundedErrorBody::from_bytes("你好世界".as_bytes(), 7);
        assert_eq!(body.text(), "你好");
        assert_eq!(body.observed_bytes(), 12);
        assert!(body.truncated());
    }

    #[tokio::test]
    async fn executor_contract_covers_stream_and_non_stream_requests_once() {
        for mode in ["stream", "non_stream"] {
            let server =
                TestServer::start("HTTP/1.1 502 Bad Gateway\r\ncontent-length: 4\r\n\r\noops")
                    .await;
            let url = server.url();
            let mut context = test_context(&url);
            context.api = mode;

            let failure = HttpAttemptExecutor::execute(
                reqwest::Client::new().get(&url),
                &context,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap_err();

            let HttpAttemptFailure::Http {
                kind: HttpFailureKind::Server,
                ..
            } = failure
            else {
                panic!("expected http failure")
            };
        }
    }

    #[tokio::test]
    async fn executor_returns_success_response_for_decoder() {
        let server = TestServer::start(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-request-id: ok-1\r\ncontent-length: 11\r\n\r\n{\"ok\":true}",
        )
        .await;
        let url = server.url();
        let context = test_context(&url);

        let success = HttpAttemptExecutor::execute(
            reqwest::Client::new().get(&url),
            &context,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            success.response.headers().get("x-request-id").unwrap(),
            "ok-1"
        );
        assert_eq!(success.response.text().await.unwrap(), "{\"ok\":true}");
    }

    #[tokio::test]
    async fn executor_returns_bounded_http_failure_with_safe_headers() {
        let body = "x".repeat(ERROR_BODY_LIMIT + 128);
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\nretry-after: 3\r\nx-request-id: rate-1\r\ncontent-length: {}\r\n\r\n{}",
            body.len(), body
        );
        let server = TestServer::start(&response).await;
        let url = server.url();
        let context = test_context(&url);

        let failure = HttpAttemptExecutor::execute(
            reqwest::Client::new().get(&url),
            &context,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        let HttpAttemptFailure::Http { body, headers, .. } = failure else {
            panic!("expected http failure")
        };
        assert_eq!(body.text().len(), ERROR_BODY_LIMIT);
        assert!(body.truncated());
        assert!(body.read_error().is_none());
        assert_eq!(headers.retry_after_ms(), Some(3_000));
    }

    /// Corrected per review finding #1 (see
    /// `body_stream_failure_after_terminal_status_preserves_http_kind_not_network`
    /// below): once a non-2xx status line has already been observed, an
    /// interrupted body read must surface as `HttpAttemptFailure::Http`
    /// preserving that status/kind — never reclassified as a generic
    /// `Network` failure that discards it. This test previously asserted
    /// exactly that (now-corrected) reclassification for a 502; it now
    /// asserts the corrected contract, including the partial/truncated body
    /// and the optional read-error diagnostic.
    #[tokio::test]
    async fn body_stream_failure_after_terminal_status_preserves_http_kind_for_502() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut request).await;
            // Advertise more bytes than are ever written, then drop the
            // connection so the body stream itself yields an error after a
            // non-success status line has already been observed.
            let head = "HTTP/1.1 502 Bad Gateway\r\ncontent-length: 64\r\n\r\n";
            tokio::io::AsyncWriteExt::write_all(&mut socket, head.as_bytes())
                .await
                .unwrap();
            tokio::io::AsyncWriteExt::write_all(&mut socket, b"partial")
                .await
                .unwrap();
            // Dropping `socket` here closes the connection abruptly.
        });
        let url = format!("http://{address}");
        let context = test_context(&url);

        let failure = HttpAttemptExecutor::execute(
            reqwest::Client::new().get(&url),
            &context,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        let HttpAttemptFailure::Http {
            status, kind, body, ..
        } = failure
        else {
            panic!("expected an Http failure preserving the already-observed 502 status, got {failure:?}")
        };
        assert_eq!(status.as_u16(), 502);
        assert_eq!(kind, HttpFailureKind::Server);
        assert_eq!(body.text(), "partial");
        assert!(body.truncated());
        assert!(
            body.read_error().is_some(),
            "an interrupted body read must carry an optional read-error diagnostic"
        );
    }

    #[tokio::test]
    async fn network_failure_log_emits_unified_receipt_without_panicking() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::ZERO)
            .build()
            .unwrap();
        let context = test_context("http://127.0.0.1:9");

        let failure = HttpAttemptExecutor::execute(
            client.get("http://127.0.0.1:9"),
            &context,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        let HttpAttemptFailure::Network { kind, .. } = &failure else {
            panic!("expected a network failure, got {failure:?}")
        };
        assert!(matches!(
            kind,
            NetworkFailureKind::Timeout | NetworkFailureKind::Connect
        ));
        failure.log(AttemptDisposition::FinalFailure);
    }

    #[tokio::test]
    async fn http_failure_log_emits_unified_receipt_without_panicking() {
        let server = TestServer::start(
            "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 4\r\n\r\noops",
        )
        .await;
        let url = server.url();
        let context = test_context(&url);

        let failure = HttpAttemptExecutor::execute(
            reqwest::Client::new().get(&url),
            &context,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(failure, HttpAttemptFailure::Http { .. }));
        // Exercises the single logging call site a migrated driver would use
        // in place of hand-built `ErrorLogContext` reassembly.
        failure.log(AttemptDisposition::FinalFailure);
    }

    #[tokio::test]
    async fn executor_cancellation_has_no_diagnostic_receipt() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let accepted = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let cancel = tokio_util::sync::CancellationToken::new();
        let child = cancel.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            child.cancel();
        });

        let failure = HttpAttemptExecutor::execute(
            reqwest::Client::new().get(&url),
            &test_context(&url),
            &cancel,
        )
        .await
        .unwrap_err();

        // `Cancelled` is a unit variant: it structurally carries no
        // `DiagnosticReceipt`, so there is nothing to log for it.
        assert!(matches!(failure, HttpAttemptFailure::Cancelled));
        accepted.abort();
    }

    fn test_context(endpoint: &str) -> HttpAttemptContext<'_> {
        HttpAttemptContext {
            driver: "test",
            api: "contract",
            provider: "test",
            model: "test-model",
            method: "GET",
            endpoint,
            attempt: 1,
            max_attempts: 2,
            message_count: 1,
            tool_count: 0,
            request_bytes: 10,
        }
    }

    struct TestServer {
        address: std::net::SocketAddr,
    }

    impl TestServer {
        async fn start(response: &str) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let response = response.to_string();
            tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut request).await;
                tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes())
                    .await
                    .unwrap();
            });
            Self { address }
        }

        fn url(&self) -> String {
            format!("http://{}", self.address)
        }
    }

    #[test]
    fn attempt_disposition_controls_retryability_and_log_level() {
        assert!(!AttemptDisposition::FinalFailure.retryable());
        assert_eq!(
            AttemptDisposition::FinalFailure.log_level(),
            log::Level::Error
        );
    }

    /// Review finding #1: once a non-success status line has already been
    /// observed (400/413/429/5xx), an interrupted body read must still
    /// surface as `HttpAttemptFailure::Http` carrying the already-known
    /// status/kind — not be silently reclassified as a generic `Network`
    /// failure that throws away the status entirely.
    ///
    /// `read_error_body` currently converts *any* body-stream error into
    /// `HttpAttemptFailure::Network` (see the existing
    /// `body_stream_failure_receipt_disposition_comes_from_context` test,
    /// which asserts exactly that reclassification for a 502). This test
    /// captures the desired contract across the specific statuses called
    /// out in the review and is expected to fail against the current
    /// implementation.
    #[tokio::test]
    async fn body_stream_failure_after_terminal_status_preserves_http_kind_not_network() {
        for (status_line, expected_kind) in [
            ("400 Bad Request", HttpFailureKind::Client),
            ("413 Payload Too Large", HttpFailureKind::ContextTooLong),
            ("429 Too Many Requests", HttpFailureKind::RateLimited),
            ("500 Internal Server Error", HttpFailureKind::Server),
        ] {
            let expected_status_code: u16 = status_line
                .split_whitespace()
                .next()
                .and_then(|code| code.parse().ok())
                .expect("status line starts with a numeric code");

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let head = format!("HTTP/1.1 {status_line}\r\ncontent-length: 64\r\n\r\n");
            tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut request).await;
                // Deliver the terminal status line and headers in full, then
                // advertise more body bytes than are ever written and drop
                // the connection — the body stream itself errors out only
                // *after* the status/headers were already observed.
                tokio::io::AsyncWriteExt::write_all(&mut socket, head.as_bytes())
                    .await
                    .unwrap();
                tokio::io::AsyncWriteExt::write_all(&mut socket, b"partial")
                    .await
                    .unwrap();
                // Dropping `socket` here closes the connection abruptly.
            });
            let url = format!("http://{address}");
            let context = test_context(&url);

            let failure = HttpAttemptExecutor::execute(
                reqwest::Client::new().get(&url),
                &context,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap_err();

            match failure {
                HttpAttemptFailure::Http { status, kind, .. } => {
                    assert_eq!(
                        status.as_u16(),
                        expected_status_code,
                        "status_line={status_line}: expected the originally observed status to be preserved"
                    );
                    assert_eq!(
                        kind, expected_kind,
                        "status_line={status_line}: expected the originally classified kind to be preserved"
                    );
                }
                other => panic!(
                    "status_line={status_line}: expected HttpAttemptFailure::Http preserving the \
                     already-observed status/kind after a body-stream interruption, got {other:?} \
                     instead — a body read failure after a terminal status must not be \
                     reclassified as a generic Network failure"
                ),
            }
        }
    }
}
