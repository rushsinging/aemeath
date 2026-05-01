//! OpenAI 兼容 provider 实现
//! 使用 OpenAIProviderConfig 替代旧 Provider enum

mod message_conversion;
mod non_stream;
mod stream;

use async_trait::async_trait;
use aemeath_core::message::Message;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use std::error::Error as StdError;
use tokio_util::sync::CancellationToken;

use crate::client::OpenAIProviderConfig;
use crate::provider::{LlmProvider, StreamHandler};
use crate::types::SystemBlock;

pub(crate) use stream::parse_openai_stream;

pub struct OpenAICompatibleProvider {
    config: OpenAIProviderConfig,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    user_agent: String,
    http: reqwest::Client,
    max_retries: u32,
    timeout_secs: u64,
    reasoning: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reasoning_effort: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl OpenAICompatibleProvider {
    pub fn new(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        Self {
            base_url: {
                let url = base_url.unwrap_or_else(|| "https://api.openai.com".to_string());
                url.trim_end_matches('/').trim_end_matches("/v1").to_string()
            },
            model: model.unwrap_or_else(|| "gpt-4o".to_string()),
            config,
            api_key,
            max_tokens,
            user_agent: format!("aemeath/{}", env!("CARGO_PKG_VERSION")),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs: 120,
            reasoning: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(reasoning)),
            reasoning_effort: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn reasoning_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.reasoning.clone()
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(secs))
            .build()
            .expect("failed to create HTTP client with custom timeout");
        self
    }

    fn chat_url(&self) -> String {
        format!("{}{}", self.base_url, self.config.chat_api_suffix)
    }

    fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", self.api_key))
            .map_err(|e| crate::LlmError::Config(e.to_string()))?);

        if self.config.is_openrouter {
            headers.insert("HTTP-Referer", HeaderValue::from_static("https://github.com/aemeath"));
        }

        headers.insert(USER_AGENT, HeaderValue::from_str(&self.user_agent)
            .map_err(|e| crate::LlmError::Config(e.to_string()))?);
        Ok(headers)
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<crate::types::StreamResponse, crate::LlmError> {
        let openai_messages = self.convert_messages(system, messages)?;
        let tools = Self::convert_tools(tool_schemas);

        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": openai_messages,
            "max_tokens": self.max_tokens,
            "stream": true,
            "stream_options": { "include_usage": true }
        });

        let reasoning_enabled = self.reasoning.load(std::sync::atomic::Ordering::Relaxed);
        if self.config.is_deepseek || self.config.is_zhipu {
            let thinking_type = if reasoning_enabled { "enabled" } else { "disabled" };
            request_body["thinking"] = serde_json::json!({"type": thinking_type});
        } else if self.config.supports_enable_thinking {
            request_body["enable_thinking"] = serde_json::json!(reasoning_enabled);
        }

        // OpenAI GPT-5.x / o-series: inject reasoning_effort
        if self.config.is_openai {
            if let Ok(guard) = self.reasoning_effort.lock() {
                if let Some(ref effort) = *guard {
                    if aemeath_core::config::models::supports_reasoning_effort(&self.model) {
                        request_body["reasoning_effort"] = serde_json::json!(effort);
                    } else {
                        log::debug!(
                            "[openai-compat] reasoning_effort='{}' set but model '{}' does not support it, ignoring",
                            effort, self.model
                        );
                    }
                }
            }
        }

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
        }

        if let Some(msgs) = request_body.get("messages").and_then(|m| m.as_array()) {
            let mut summary = String::with_capacity(256);
            for (i, m) in msgs.iter().enumerate() {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("?");
                match role {
                    "assistant" => {
                        let has_tc = m.get("tool_calls").is_some();
                        let rc_len = m.get("reasoning_content")
                            .and_then(|r| r.as_str())
                            .map(|s| s.len() as i32)
                            .unwrap_or(-1);
                        let content_null = m.get("content").map(|c| c.is_null()).unwrap_or(false);
                        summary.push_str(&format!(
                            "\n  [{i}] assistant rc_len={rc_len} tc={has_tc} content_null={content_null}"
                        ));
                    }
                    "tool" => {
                        let tcid = m.get("tool_call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tcid_short: String = tcid.chars().take(24).collect();
                        summary.push_str(&format!("\n  [{i}] tool id={tcid_short}"));
                    }
                    _ => {
                        summary.push_str(&format!("\n  [{i}] {role}"));
                    }
                }
            }
            let body_bytes = serde_json::to_string(&request_body)
                .map(|s| s.len())
                .unwrap_or(0);
            log::debug!(
                "[openai-compat stream] POST provider={} body_bytes={} messages={}:{}",
                self.config.provider_name,
                body_bytes,
                msgs.len(),
                summary,
            );
        }

        let headers = self.build_headers()?;

        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }

            if attempt > 0 {
                let delay = std::time::Duration::from_millis((1000 * 2u64.pow(attempt as u32)).min(30_000));
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        return Err(crate::LlmError::Stream("interrupted by user".to_string()));
                    }
                    _ = tokio::time::sleep(delay) => {}
                }
            }

            let send_fut = self
                .http
                .post(self.chat_url())
                .headers(headers.clone())
                .json(&request_body)
                .send();

            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(crate::LlmError::Stream("interrupted by user".to_string()));
                }
                result = send_fut => {
                    match result {
                        Ok(resp) => resp,
                        Err(e) => {
                            let url = self.chat_url();
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
                            let mut source: Option<&dyn StdError> = StdError::source(&e);
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
                        attempt + 2, self.max_retries
                    ));
                }
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                if remaining > 0 {
                    handler.on_error(&format!(
                        "server error ({}), retrying ({}/{})...",
                        status, attempt + 2, self.max_retries
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

            match parse_openai_stream(response, handler, cancel).await {
                Ok(resp) => return Ok(resp),
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(e)) => {
                    handler.on_error(&format!("Streaming error: {}, retrying...", e));
                    last_error = Some(crate::LlmError::Stream(e));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        if let Some(ref err) = last_error {
            if matches!(err, crate::LlmError::Stream(_)) {
                handler.on_error("All streaming retries failed, attempting non-streaming fallback");
                return self.send_message_non_stream(system, messages, tool_schemas, handler).await;
            }
        }
        Err(last_error.unwrap_or(crate::LlmError::Network("max retries exceeded".to_string())))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.config.provider_name
    }

    fn set_reasoning(&self, enabled: bool) {
        self.reasoning.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_reasoning(&self) -> bool {
        self.reasoning.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_reasoning_effort(&self, effort: Option<String>) {
        if let Ok(mut guard) = self.reasoning_effort.lock() {
            *guard = effort;
        }
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.reasoning_effort.lock().ok().and_then(|g| g.clone())
    }
}
