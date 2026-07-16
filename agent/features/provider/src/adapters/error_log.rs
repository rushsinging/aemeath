//! Provider-owned LLM API error diagnostic payload and sanitization.

use serde::Serialize;

pub(crate) const LLM_API_ERROR_TARGET: &str = "aemeath:llm-api-error";
const PREVIEW_LIMIT: usize = 1_024;
const SOURCE_CHAIN_LIMIT: usize = 8;

pub(crate) struct ErrorLogContext<'a> {
    pub driver: &'a str,
    pub api: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub endpoint: &'a str,
    pub attempt: u32,
    pub max_attempts: u32,
    pub elapsed_ms: u128,
    pub message_count: usize,
    pub tool_count: usize,
    pub request_bytes: usize,
}

pub(crate) fn log_network_error(
    context: ErrorLogContext<'_>,
    error: &(dyn std::error::Error + 'static),
    retryable: bool,
) {
    let mut record = LlmApiErrorRecord::new(&context, "network_error");
    record.retryable = retryable;
    record.elapsed_ms = context.elapsed_ms;
    record.message_count = context.message_count;
    record.tool_count = context.tool_count;
    record.request_bytes = context.request_bytes;
    record.source_chain = source_chain(error);
    record.log(!retryable);
}

pub(crate) fn log_http_error(
    context: ErrorLogContext<'_>,
    status: reqwest::StatusCode,
    body: &str,
    retryable: bool,
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
    record.elapsed_ms = context.elapsed_ms;
    record.message_count = context.message_count;
    record.tool_count = context.tool_count;
    record.request_bytes = context.request_bytes;
    record.response_bytes = body.len();
    record.body_preview = Some(sanitize_preview(body));
    record.log(!retryable);
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LlmApiErrorRecord<'a> {
    pub event_type: &'static str,
    pub driver: &'a str,
    pub api: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub method: &'a str,
    pub endpoint: String,
    pub http_status: Option<u16>,
    pub provider_request_id: Option<&'a str>,
    pub error_kind: &'a str,
    pub error_code: Option<&'a str>,
    pub retryable: bool,
    pub attempt: u32,
    pub max_attempts: u32,
    pub retry_after_ms: Option<u64>,
    pub elapsed_ms: u128,
    pub message_count: usize,
    pub tool_count: usize,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub partial_output: bool,
    pub fallback_planned: bool,
    pub body_preview: Option<String>,
    pub source_chain: Vec<String>,
}

impl<'a> LlmApiErrorRecord<'a> {
    pub(crate) fn new(context: &ErrorLogContext<'a>, error_kind: &'a str) -> Self {
        Self {
            event_type: "llm_api_error",
            driver: context.driver,
            api: context.api,
            provider: context.provider,
            model: context.model,
            method: "POST",
            endpoint: sanitize_endpoint(context.endpoint),
            http_status: None,
            provider_request_id: None,
            error_kind,
            error_code: None,
            retryable: false,
            attempt: context.attempt,
            max_attempts: context.max_attempts,
            retry_after_ms: None,
            elapsed_ms: 0,
            message_count: 0,
            tool_count: 0,
            request_bytes: 0,
            response_bytes: 0,
            partial_output: false,
            fallback_planned: false,
            body_preview: None,
            source_chain: Vec::new(),
        }
    }

    pub(crate) fn log(self, final_failure: bool) {
        let payload = serde_json::to_string(&self).unwrap_or_else(|_| {
            r#"{"event_type":"llm_api_error","error_kind":"serialization"}"#.to_string()
        });
        if final_failure {
            log::error!(target: LLM_API_ERROR_TARGET, "{payload}");
        } else {
            log::debug!(target: LLM_API_ERROR_TARGET, "{payload}");
        }
    }
}

pub(crate) fn sanitize_endpoint(raw: &str) -> String {
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

pub(crate) fn sanitize_preview(raw: &str) -> String {
    let truncated: String = raw.chars().take(PREVIEW_LIMIT).collect();
    let mut value = match serde_json::from_str::<serde_json::Value>(&truncated) {
        Ok(mut value) => {
            redact_json(&mut value);
            value.to_string()
        }
        Err(_) => redact_text(&truncated),
    };
    if raw.chars().count() > PREVIEW_LIMIT {
        value.push('…');
    }
    value
}

pub(crate) fn source_chain(error: &(dyn std::error::Error + 'static)) -> Vec<String> {
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

fn redact_text(raw: &str) -> String {
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
}
