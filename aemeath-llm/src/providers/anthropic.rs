//! Anthropic Claude provider implementation

use async_trait::async_trait;
use aemeath_core::message::Message;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use tokio_util::sync::CancellationToken;

use crate::provider::{LlmProvider, StreamHandler};
use crate::stream::parse_stream;
use crate::types::{CreateMessageRequest, StreamResponse, SystemBlock};

/// Handler wrapper that tracks whether any user-visible content was emitted.
/// Used to decide if a non-stream fallback is safe on stream errors — if any
/// text/tool_use was already shown, falling back would duplicate it.
struct TrackingHandler<'a> {
    inner: &'a mut dyn StreamHandler,
    emitted: bool,
}

impl<'a> TrackingHandler<'a> {
    fn new(inner: &'a mut dyn StreamHandler) -> Self {
        Self { inner, emitted: false }
    }
}

impl<'a> StreamHandler for TrackingHandler<'a> {
    fn on_text(&mut self, text: &str) {
        self.emitted = true;
        self.inner.on_text(text);
    }
    fn on_tool_use_start(&mut self, name: &str) {
        self.emitted = true;
        self.inner.on_tool_use_start(name);
    }
    fn on_error(&mut self, error: &str) {
        self.inner.on_error(error);
    }
    fn on_raw_line(&mut self, line: &str) {
        self.inner.on_raw_line(line);
    }
    fn on_text_block_complete(&mut self, full_text: &str) {
        self.inner.on_text_block_complete(full_text);
    }
    fn on_thinking(&mut self, text: &str) {
        self.emitted = true;
        self.inner.on_thinking(text);
    }
}

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    user_agent: String,
    http: reqwest::Client,
    /// Maximum retry attempts (default 3)
    max_retries: u32,
    /// Request timeout in seconds (default 60)
    timeout_secs: u64,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: Option<String>, model: Option<String>, max_tokens: u32) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            model: model.unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
            max_tokens,
            user_agent: format!("aemeath/{}", env!("CARGO_PKG_VERSION")),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs: 120,
        }
    }

    /// Set maximum retry attempts
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set request timeout in seconds
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_str(&self.api_key)
            .map_err(|e| crate::LlmError::Config(e.to_string()))?);
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert("anthropic-beta", HeaderValue::from_static("prompt-caching-2024-07-31"));
        headers.insert(USER_AGENT, HeaderValue::from_str(&self.user_agent)
            .map_err(|e| crate::LlmError::Config(e.to_string()))?);
        Ok(headers)
    }

    async fn send_message_non_stream(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
    ) -> Result<StreamResponse, crate::LlmError> {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();

        let request = CreateMessageRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system.to_vec(),
            messages: api_messages,
            tools: tool_schemas.to_vec(),
            stream: false,
        };

        let headers = self.build_headers()?;

        let url = format!("{}/v1/messages", self.base_url);
        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                let mut msg = format!("{}\n  URL: {}", e, url);
                let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                let mut depth = 1;
                while let Some(cause) = source {
                    msg.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                    source = cause.source();
                    depth += 1;
                }
                crate::LlmError::Network(msg)
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(crate::LlmError::Api {
                error_type: status.to_string(),
                message: body,
            });
        }

        let body: serde_json::Value = response.json().await
            .map_err(|e| crate::LlmError::Stream(e.to_string()))?;

        // Parse the non-streaming response into StreamResponse
        let mut content_blocks = Vec::new();
        if let Some(content) = body.get("content").and_then(|v| v.as_array()) {
            for block in content {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            handler.on_text(text);
                            handler.on_text_block_complete(text);
                            content_blocks.push(aemeath_core::message::ContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                    "tool_use" => {
                        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let input = block.get("input").cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                        handler.on_tool_use_start(&name);
                        content_blocks.push(aemeath_core::message::ContentBlock::ToolUse { id, name, input });
                    }
                    _ => {}
                }
            }
        }

        let usage = crate::types::Usage {
            input_tokens: body.get("usage").and_then(|u| u.get("input_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            output_tokens: body.get("usage").and_then(|u| u.get("output_tokens")).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        };

        let stop_reason_str = body.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("end_turn");

        Ok(StreamResponse {
            assistant_message: aemeath_core::message::Message {
                role: aemeath_core::message::Role::Assistant,
                content: content_blocks,
            },
            usage,
            stop_reason: crate::types::StopReason::from_str(stop_reason_str),
        })
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

        let request = CreateMessageRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system.to_vec(),
            messages: api_messages,
            tools: tool_schemas.to_vec(),
            stream: true,
        };

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
                .post(format!("{}/v1/messages", self.base_url))
                .headers(headers.clone())
                .json(&request)
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
                        attempt + 2, self.max_retries
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
                    return self.send_message_non_stream(system, messages, tool_schemas, handler).await;
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
}
