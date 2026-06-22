//! Anthropic Claude provider implementation

mod message_conversion;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::business::stream::parse_stream;
use crate::business::types::{CreateMessageRequest, StreamResponse, SystemBlock};
use crate::core::provider::{LlmProvider, StreamHandler};

use message_conversion::{send_message_non_stream, RequestParams, TrackingHandler};

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: Arc<AtomicU32>,
    thinking_max_tokens: Arc<AtomicU32>,
    user_agent: String,
    http: reqwest::Client,
    /// Maximum retry attempts (default 3)
    max_retries: u32,
    /// Request timeout in seconds (default 60)
    /// 仅由未消费的 builder `with_timeout_secs` 写入，收窄可见性后暴露为孤儿，保留备用（refs #61 D3）。
    #[allow(dead_code)]
    timeout_secs: u64,
}

impl AnthropicProvider {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        thinking_max_tokens: u32,
    ) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
            max_tokens: Arc::new(AtomicU32::new(max_tokens)),
            thinking_max_tokens: Arc::new(AtomicU32::new(thinking_max_tokens)),
            user_agent: format!("aemeath/{}", share::version()),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs: 120,
        }
    }

    /// Set maximum retry attempts
    /// builder 方法当前无调用点，收窄可见性后暴露为孤儿，保留备用（refs #61 D3）。
    #[allow(dead_code)]
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set request timeout in seconds
    /// builder 方法当前无调用点，收窄可见性后暴露为孤儿，保留备用（refs #61 D3）。
    #[allow(dead_code)]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(secs))
            .build()
            .expect("failed to create HTTP client with custom timeout");
        self
    }

    pub(crate) fn current_max_tokens(&self) -> u32 {
        self.max_tokens.load(Ordering::Relaxed)
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
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();

        // 为 tools 数组最后一个元素添加 cache_control 断点，让 Anthropic
        // 缓存整个 tools schema（≈6K tokens）。后续 turn 命中 cache 后
        // 固定开销成本降至约 1/10。Anthropic 原生支持 tools 数组缓存。
        let mut cached_tools = tool_schemas.to_vec();
        if let Some(last_tool) = cached_tools.last_mut() {
            if let Some(obj) = last_tool.as_object_mut() {
                obj.insert(
                    "cache_control".to_string(),
                    serde_json::json!({"type": "ephemeral"}),
                );
            }
        }
        let request = CreateMessageRequest::new(
            self.model.clone(),
            self.current_max_tokens(),
            self.thinking_max_tokens.load(Ordering::Relaxed),
            system.to_vec(),
            api_messages,
            cached_tools,
            true,
        );

        let headers = self.build_headers()?;

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

            let send_fut = self
                .http
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers.clone())
                .json(&request.clone().into_json())
                .send();

            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(crate::LlmError::Cancelled);
                }
                result = send_fut => {
                    match result {
                        Ok(resp) => resp,
                        Err(e) => {
                            let url = format!("{}/v1/messages", self.base_url);
                            let detail = if e.is_connect() {
                                "connection failed"
                            } else if e.is_timeout() {
                                "request timed out"
                            } else if e.is_redirect() {
                                "too many redirects"
                            } else if e.is_request() {
                                "request build error"
                            } else if e.is_body() {
                                "request body error"
                            } else if e.is_decode() {
                                "response decode error"
                            } else {
                                "unknown"
                            };
                            let mut msg = format!("{} ({})\n  URL: {}", e, detail, url);
                            let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                            let mut depth = 1;
                            while let Some(cause) = source {
                                msg.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                                source = cause.source();
                                depth += 1;
                            }
                            let remaining = self.max_retries.saturating_sub(attempt + 1);
                            if remaining > 0 {
                                handler.on_error(&format!(
                                    "network error ({detail}), retrying ({}/{})...",
                                    attempt + 2, self.max_retries
                                ));
                            }
                            last_error = Some(crate::LlmError::Network(msg));
                            continue;
                        }
                    }
                }
            };

            let status = response.status();
            if status == 429 {
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                if remaining > 0 {
                    handler.on_error(&format!(
                        "rate limited (429), retrying ({}/{})...",
                        attempt + 2,
                        self.max_retries
                    ));
                }
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            // Retry 5xx errors (server-side issues may be transient)
            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                let remaining = self.max_retries.saturating_sub(attempt + 1);
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
                    message: error_body,
                });
                continue;
            }

            if status == 413 {
                return Err(crate::LlmError::ContextTooLong);
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: body,
                });
            }

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
                    let params = RequestParams {
                        model: self.model.clone(),
                        max_tokens: self.current_max_tokens(),
                        thinking_max_tokens: self.thinking_max_tokens.load(Ordering::Relaxed),
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

    fn set_max_tokens(&self, max_tokens: u32) {
        if max_tokens > 0 {
            self.max_tokens.store(max_tokens, Ordering::Relaxed);
        }
    }

    fn max_tokens(&self) -> u32 {
        self.current_max_tokens()
    }

    fn set_reasoning_level(&self, level: crate::core::provider::ReasoningLevel) {
        use crate::core::provider::ReasoningLevel;
        let tokens: u32 = match level {
            ReasoningLevel::Off => 0,
            ReasoningLevel::Low => 1024,
            ReasoningLevel::Medium => 4096,
            ReasoningLevel::High => 16384,
            ReasoningLevel::Xhigh => 32768,
            ReasoningLevel::Max => 65536,
        };
        self.thinking_max_tokens.store(tokens, Ordering::Relaxed);
    }

    fn current_reasoning_level(&self) -> crate::core::provider::ReasoningLevel {
        use crate::core::provider::ReasoningLevel;
        match self.thinking_max_tokens.load(Ordering::Relaxed) {
            0 => ReasoningLevel::Off,
            1..=1024 => ReasoningLevel::Low,
            1025..=4096 => ReasoningLevel::Medium,
            4097..=16384 => ReasoningLevel::High,
            16385..=32768 => ReasoningLevel::Xhigh,
            _ => ReasoningLevel::Max,
        }
    }

    fn max_reasoning_level(&self) -> crate::core::provider::ReasoningLevel {
        crate::core::provider::ReasoningLevel::Max
    }
}

#[cfg(test)]
mod tests {
    use crate::business::types::CreateMessageRequest;

    #[test]
    fn anthropic_request_serializes_thinking_budget() {
        let request = CreateMessageRequest::new(
            "claude-sonnet-4-6".to_string(),
            8192,
            4096,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
        );

        let value = request.into_json();
        assert_eq!(
            value.get("thinking").unwrap().get("type"),
            Some(&serde_json::json!("enabled"))
        );
        assert_eq!(
            value.get("thinking").unwrap().get("budget_tokens"),
            Some(&serde_json::json!(4096))
        );
    }

    #[test]
    fn anthropic_request_omits_thinking_when_budget_zero() {
        let request = CreateMessageRequest::new(
            "claude-sonnet-4-6".to_string(),
            8192,
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            true,
        );

        let value = request.into_json();
        assert!(value.get("thinking").is_none());
    }
}
