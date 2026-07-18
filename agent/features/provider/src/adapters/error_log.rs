//! Provider-owned LLM API error diagnostic payload and sanitization.
//!
//! This module intentionally exposes a narrow logging surface:
//! - [`log_stream_protocol_error`] is the only entry point any driver should
//!   call directly. It covers stream/protocol-level failures (SSE parsing,
//!   schema drift, truncated upstream output, ...) that never touch HTTP
//!   transport at all, so `HttpAttemptExecutor` cannot see them.
//! - [`log_network_error`] and [`log_http_error`] back
//!   `HttpAttemptFailure::log` (see `http_attempt.rs`) and exist purely to
//!   serve that single call site. No other adapter may call them — drivers
//!   migrated onto `HttpAttemptExecutor` get this HTTP/network diagnostic
//!   logging automatically through `failure.log()`.

use serde::Serialize;

use super::http_attempt::{BoundedErrorBody, SafeResponseHeaders};

pub(crate) const LLM_API_ERROR_TARGET: &str = "aemeath:llm-api-error";
const PREVIEW_LIMIT: usize = 1_024;
const SOURCE_CHAIN_LIMIT: usize = 8;

pub(crate) struct ErrorLogContext<'a> {
    pub driver: &'a str,
    pub api: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub method: &'a str,
    pub endpoint: &'a str,
    pub attempt: u32,
    pub max_attempts: u32,
    pub elapsed_ms: u128,
    pub message_count: usize,
    pub tool_count: usize,
    pub request_bytes: usize,
}

/// HTTP transport (connect/timeout/decode/...) failure diagnostics.
///
/// Only `http_attempt.rs` may call this — it exists solely to back
/// [`super::http_attempt::HttpAttemptFailure::log`]. Drivers must never
/// construct this payload themselves; route through `HttpAttemptExecutor`
/// and call `failure.log()` instead.
pub(crate) fn log_network_error(
    context: ErrorLogContext<'_>,
    error: &(dyn std::error::Error + 'static),
    retryable: bool,
    level: log::Level,
) {
    let mut record = LlmApiErrorRecord::new(&context, "network_error");
    record.retryable = retryable;
    record.source_chain = source_chain(error);
    record.log(level);
}

/// Non-2xx HTTP response diagnostics.
///
/// Only `http_attempt.rs` may call this — it exists solely to back
/// [`super::http_attempt::HttpAttemptFailure::log`]. Drivers must never
/// construct this payload themselves; route through `HttpAttemptExecutor`
/// and call `failure.log()` instead.
pub(crate) fn log_http_error(
    context: ErrorLogContext<'_>,
    status: reqwest::StatusCode,
    headers: &SafeResponseHeaders,
    body: &BoundedErrorBody,
    retryable: bool,
    level: log::Level,
) {
    let error_kind = if status.is_server_error() {
        "http_server_error"
    } else {
        "http_client_error"
    };
    let mut record = LlmApiErrorRecord::new(&context, error_kind);
    record.http_status = Some(status.as_u16());
    record.error_code = Some(if status.is_server_error() {
        "http_5xx"
    } else {
        "http_non_success"
    });
    record.retryable = retryable;
    record.provider_request_id = headers.provider_request_id();
    record.retry_after_ms = headers.retry_after_ms();
    record.response_content_type = headers.content_type();
    record.response_bytes = body.observed_bytes();
    record.response_truncated = body.truncated();
    record.body_preview = Some(sanitize_preview(body.text()));
    record.body_read_error = body.read_error().map(redact_text);
    record.log(level);
}

/// Stream/protocol-level failure diagnostics (SSE parsing, schema drift,
/// truncated upstream output, ...). These never touch HTTP transport, so
/// `HttpAttemptExecutor` cannot observe or log them itself. This is the
/// narrow `error_log` API drivers should call directly.
pub(crate) fn log_stream_protocol_error(
    context: ErrorLogContext<'_>,
    body: &str,
    retryable: bool,
    fallback_planned: bool,
    level: log::Level,
) {
    let mut record = LlmApiErrorRecord::new(&context, "stream_protocol_error");
    record.retryable = retryable;
    record.partial_output = true;
    record.fallback_planned = fallback_planned;
    record.body_preview = Some(sanitize_preview(body));
    record.log(level);
}

#[derive(Debug, Clone, Serialize)]
struct LlmApiErrorRecord<'a> {
    event_type: &'static str,
    driver: &'a str,
    api: &'a str,
    provider: &'a str,
    model: &'a str,
    method: &'a str,
    endpoint: String,
    http_status: Option<u16>,
    provider_request_id: Option<&'a str>,
    error_kind: &'a str,
    error_code: Option<&'a str>,
    retryable: bool,
    attempt: u32,
    max_attempts: u32,
    retry_after_ms: Option<u64>,
    elapsed_ms: u128,
    message_count: usize,
    tool_count: usize,
    request_bytes: usize,
    response_bytes: usize,
    response_content_type: Option<&'a str>,
    response_truncated: bool,
    /// Diagnostic detail when the body read itself was interrupted
    /// mid-stream after a non-2xx status was already observed (see
    /// `BoundedErrorBody::read_error`). `None` for a body read to
    /// completion.
    body_read_error: Option<String>,
    partial_output: bool,
    fallback_planned: bool,
    body_preview: Option<String>,
    source_chain: Vec<String>,
}

impl<'a> LlmApiErrorRecord<'a> {
    fn new(context: &ErrorLogContext<'a>, error_kind: &'a str) -> Self {
        Self {
            event_type: "llm_api_error",
            driver: context.driver,
            api: context.api,
            provider: context.provider,
            model: context.model,
            method: context.method,
            endpoint: sanitize_endpoint(context.endpoint),
            http_status: None,
            provider_request_id: None,
            error_kind,
            error_code: None,
            retryable: false,
            attempt: context.attempt,
            max_attempts: context.max_attempts,
            retry_after_ms: None,
            elapsed_ms: context.elapsed_ms,
            message_count: context.message_count,
            tool_count: context.tool_count,
            request_bytes: context.request_bytes,
            response_bytes: 0,
            response_content_type: None,
            response_truncated: false,
            body_read_error: None,
            partial_output: false,
            fallback_planned: false,
            body_preview: None,
            source_chain: Vec::new(),
        }
    }

    fn log(self, level: log::Level) {
        let payload = serde_json::to_string(&self).unwrap_or_else(|_| {
            r#"{"event_type":"llm_api_error","error_kind":"serialization"}"#.to_string()
        });
        match level {
            log::Level::Error => log::error!(target: LLM_API_ERROR_TARGET, "{payload}"),
            log::Level::Warn => log::warn!(target: LLM_API_ERROR_TARGET, "{payload}"),
            log::Level::Info => log::info!(target: LLM_API_ERROR_TARGET, "{payload}"),
            log::Level::Debug => log::debug!(target: LLM_API_ERROR_TARGET, "{payload}"),
            log::Level::Trace => log::trace!(target: LLM_API_ERROR_TARGET, "{payload}"),
        }
    }
}

fn sanitize_endpoint(raw: &str) -> String {
    match reqwest::Url::parse(raw) {
        Ok(mut url) => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => "<invalid-endpoint>".to_string(),
    }
}

/// Renders a safe, size-bounded preview of a raw response body for logging.
///
/// Redaction must happen on the *complete* body before truncation, not the
/// other way around: truncating first and only then trying to parse/redact
/// can (a) cut a compact JSON document mid-token, so the truncated slice no
/// longer parses as JSON at all and falls through to the text-based
/// fallback, and (b) even when parsing does succeed, a secret sitting past
/// the truncation point would never be reachable for redaction. Parsing and
/// redacting the whole bounded body first means every secret gets replaced
/// regardless of where in the document it appears or how long the document
/// is; only the final, already-redacted string is ever truncated for the
/// preview.
fn sanitize_preview(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(mut value) => {
            redact_json(&mut value);
            truncate_with_ellipsis(&value.to_string())
        }
        Err(_) => truncate_with_ellipsis(&redact_text(raw)),
    }
}

fn truncate_with_ellipsis(text: &str) -> String {
    let mut truncated: String = text.chars().take(PREVIEW_LIMIT).collect();
    if text.chars().count() > PREVIEW_LIMIT {
        truncated.push('…');
    }
    truncated
}

fn source_chain(error: &(dyn std::error::Error + 'static)) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Some(error);
    while let Some(cause) = current.take() {
        if result.len() == SOURCE_CHAIN_LIMIT {
            break;
        }
        result.push(redact_text(&cause.to_string()));
        current = cause.source();
    }
    result
}

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if is_secret_key(key) {
                    *value = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_json),
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    [
        "api_key",
        "access_token",
        "authorization",
        "cookie",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|secret| normalized.contains(secret))
}

/// Redacts common secrets from raw (non-JSON, or invalid-JSON) text.
///
/// Runs two independent passes so neither text shape lets a secret slip
/// through:
/// - [`redact_quoted_key_value_pairs`] handles JSON-*like* `"key":"value"` /
///   `"key":value` pairs — the shape a body that merely *fails* strict JSON
///   parsing (invalid syntax, truncated, ...) still commonly has. This does
///   not depend on whitespace surrounding the separators, so it also covers
///   the compact (no-whitespace) JSON case that a plain token-split pass
///   would miss entirely.
/// - The whitespace-token pass below covers plain-text log/error lines such
///   as `Authorization: Bearer <token>` or `api_key=<value>` that have no
///   quoting at all.
fn redact_text(raw: &str) -> String {
    redact_whitespace_tokens(&redact_quoted_key_value_pairs(raw))
}

/// Scans `raw` left-to-right for `"key"` (optionally followed by whitespace,
/// then `:`, then whitespace) and redacts the value that follows when `key`
/// matches [`is_secret_key`]. Tolerates syntactically invalid JSON (missing
/// commas, unbalanced braces, trailing garbage, ...) since it never actually
/// parses the text as JSON — it only recognizes the local `"key":value`
/// shape wherever it appears.
fn redact_quoted_key_value_pairs(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut output = String::with_capacity(raw.len());
    let mut i = 0;
    while i < len {
        if chars[i] != '"' {
            output.push(chars[i]);
            i += 1;
            continue;
        }
        let Some(key_close) = find_closing_quote(&chars, i + 1) else {
            // Unterminated quote: copy the rest verbatim and stop.
            output.extend(&chars[i..]);
            break;
        };
        let key: String = chars[i + 1..key_close].iter().collect();
        let mut j = key_close + 1;
        while j < len && chars[j].is_whitespace() {
            j += 1;
        }
        if j >= len || chars[j] != ':' {
            // Not followed by a colon — just a quoted string, copy as-is.
            output.extend(&chars[i..=key_close]);
            i = key_close + 1;
            continue;
        }
        let mut k = j + 1;
        while k < len && chars[k].is_whitespace() {
            k += 1;
        }
        if k < len && chars[k] == '"' {
            let Some(value_close) = find_closing_quote(&chars, k + 1) else {
                output.extend(&chars[i..]);
                break;
            };
            if is_secret_key(&key) {
                output.push('"');
                output.push_str(&key);
                output.push_str("\":\"[REDACTED]\"");
            } else {
                output.extend(&chars[i..=value_close]);
            }
            i = value_close + 1;
        } else {
            let value_start = k;
            let mut m = k;
            while m < len && !matches!(chars[m], ',' | '}' | ']' | '"') {
                m += 1;
            }
            if m == value_start {
                // No bare value found; copy the key quote and move on.
                output.extend(&chars[i..=key_close]);
                i = key_close + 1;
                continue;
            }
            if is_secret_key(&key) {
                output.push('"');
                output.push_str(&key);
                output.push_str("\":[REDACTED]");
            } else {
                output.extend(&chars[i..m]);
            }
            i = m;
        }
    }
    output
}

/// Finds the index of the next unescaped `"` at or after `start`.
fn find_closing_quote(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '"' && chars[i - 1] != '\\' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn redact_whitespace_tokens(raw: &str) -> String {
    let mut redact_next = false;
    raw.split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if redact_next {
                redact_next = false;
                return "[REDACTED]".to_string();
            }
            if lower.starts_with("bearer") || lower == "authorization" || lower == "cookie" {
                redact_next = true;
                return "[REDACTED]".to_string();
            }
            if lower.starts_with("http://") || lower.starts_with("https://") {
                return sanitize_endpoint(part.trim_matches(|c: char| {
                    matches!(c, ',' | ';' | ')' | ']' | '}' | '"' | '\'')
                }));
            }
            if lower.contains("api_key=")
                || lower.contains("access_token=")
                || lower.contains("password=")
                || lower.contains("secret=")
                || lower.contains("token=")
            {
                "[REDACTED]".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_drops_credentials_query_and_fragment() {
        assert_eq!(
            sanitize_endpoint("https://user:pass@example.com/v1/chat?api_key=secret#frag"),
            "https://example.com/v1/chat"
        );
        assert_eq!(sanitize_endpoint("not a url"), "<invalid-endpoint>");
    }

    #[test]
    fn json_preview_redacts_nested_secret_fields() {
        let preview = sanitize_preview(
            r#"{"error":{"message":"bad","api_key":"sk-secret","nested":{"access_token":"token"}}}"#,
        );
        assert!(preview.contains("[REDACTED]"));
        assert!(!preview.contains("sk-secret"));
        assert!(!preview.contains("\"token\""));
    }

    #[test]
    fn text_preview_redacts_common_inline_secrets_and_truncates() {
        let input = format!(
            "authorization Bearer-secret api_key=secret {}",
            "x".repeat(2_000)
        );
        let preview = sanitize_preview(&input);
        assert!(!preview.contains("Bearer-secret"));
        assert!(!preview.contains("api_key=secret"));
        assert!(preview.ends_with('…'));
    }

    /// Review finding #5: `sanitize_preview` truncates to `PREVIEW_LIMIT`
    /// *characters* before attempting to parse/redact. For a **compact**
    /// (no whitespace) JSON body whose total length exceeds the limit, the
    /// char-level cut lands mid-document, so `serde_json::from_str` fails on
    /// the now-invalid truncated JSON and the code falls back to
    /// `redact_text`. But `redact_text` only recognizes secrets as
    /// whitespace-delimited tokens matching `key=value` (e.g.
    /// `api_key=secret`); a compact JSON body uses `"api_key":"value"`
    /// (colon, not `=`, and no surrounding whitespace to split on), so the
    /// secret survives untouched in the logged preview even though the
    /// `api_key` field sits well within the first `PREVIEW_LIMIT`
    /// characters of the raw body.
    #[test]
    fn compact_json_preview_over_limit_still_redacts_leading_api_key() {
        let secret = "sk-super-secret-0123456789";
        // Filler pushes the *overall* document past PREVIEW_LIMIT while the
        // `api_key` field itself sits near the very start, well inside the
        // truncation window.
        let filler = "y".repeat(2_000);
        let raw =
            format!(r#"{{"api_key":"{secret}","message":"error detail","filler":"{filler}"}}"#);
        assert!(
            raw.chars().count() > PREVIEW_LIMIT,
            "fixture must exceed the preview truncation limit"
        );
        let key_pos = raw.find("api_key").expect("fixture contains api_key");
        assert!(
            key_pos < PREVIEW_LIMIT,
            "api_key must sit inside the first PREVIEW_LIMIT characters"
        );

        let preview = sanitize_preview(&raw);

        assert!(
            !preview.contains(secret),
            "api_key leaked in a truncated compact-JSON preview even though the field was \
             well within the first {PREVIEW_LIMIT} characters of the raw body: {preview}"
        );
    }
}
