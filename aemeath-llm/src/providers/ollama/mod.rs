//! Ollama provider implementation — 主模块
//! 本地 Ollama 推理服务优化：更长超时、可选认证、无 stream_options、空响应检测。

use async_trait::async_trait;
use aemeath_core::message::Message;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use tokio_util::sync::CancellationToken;

use crate::provider::{LlmProvider, StreamHandler};
use crate::types::{StreamResponse, SystemBlock};

mod conversion;
mod non_stream;
mod stream;

use conversion::OllamaProviderConversion;
use non_stream::OllamaProviderNonStream;
use stream::parse_ollama_stream;

pub struct OllamaProvider {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) max_tokens: u32,
    /// If false, send `think: false` to disable reasoning mode for models
    /// that support it (qwen3, deepseek-r1, gpt-oss, etc.)
    pub(crate) reasoning: bool,
    pub(crate) user_agent: String,
    pub(crate) http: reqwest::Client,
    pub(crate) max_retries: u32,
    pub(crate) timeout_secs: u64,
}

/// Default request timeout for Ollama (5 minutes) — model loading can be slow
pub(crate) const DEFAULT_TIMEOUT_SECS: u64 = 300;
/// Stream idle timeout: abort if no data for 3 minutes (Ollama may stall during generation)
pub(crate) const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

impl OllamaProvider {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        Self {
            base_url: {
                let url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
                url.trim_end_matches('/').trim_end_matches("/v1").to_string()
            },
            model: model.unwrap_or_else(|| "llama3.2".to_string()),
            api_key,
            max_tokens,
            reasoning,
            user_agent: format!("aemeath/{}", env!("CARGO_PKG_VERSION")),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
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

    fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Ollama doesn't require auth, but send it if provided (for proxy setups)
        if !self.api_key.is_empty() && self.api_key != "ollama" {
            headers.insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| crate::LlmError::Config(e.to_string()))?,
            );
        }
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );
        Ok(headers)
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let request_body = self.build_request_body(system, messages, tool_schemas, true)?;
        let headers = self.build_headers()?;
        let url = format!("{}/api/chat", self.base_url);

        let body_bytes = serde_json::to_string(&request_body).map(|s| s.len()).unwrap_or(0);
        log::debug!(
            "[ollama stream] POST {} model={} think={} msgs={} tools={} body_bytes={}",
            url,
            self.model,
            self.reasoning,
            messages.len(),
            tool_schemas.len(),
            body_bytes,
        );

        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }

            if attempt > 0 {
                let delay = std::time::Duration::from_millis((1000 * 2u64.pow(attempt as u32)).min(30_000));
                log::debug!(
                    "[ollama stream] retry {}/{} after {:?}",
                    attempt, self.max_retries, delay
                );
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
                .post(&url)
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
                        Ok(r) => r,
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("timed out") || msg.contains("timeout") {
                                // Ollama request timed out — will retry
                                last_error = Some(crate::LlmError::Network(format!(
                                    "Ollama request timed out after {}s — is the model loaded?", self.timeout_secs
                                )));
                                continue;
                            }
                            let mut detailed = format!("{}\n  URL: {}", e, url);
                            let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                            let mut depth = 1;
                            while let Some(cause) = source {
                                detailed.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                                source = cause.source();
                                depth += 1;
                            }
                            return Err(crate::LlmError::Network(detailed));
                        }
                    }
                }
            };

            let status = response.status();
            log::debug!(
                "[ollama stream] attempt={} HTTP {} content-type={:?}",
                attempt,
                status,
                response.headers().get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
            );

            if status == 429 {
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                log::debug!("[ollama stream] 5xx body: {}", error_body);
                last_error = Some(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: error_body,
                });
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                log::debug!("[ollama stream] non-success body: {}", body);
                return Err(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: body,
                });
            }

            match parse_ollama_stream(response, handler, cancel).await {
                Ok(resp) => {
                    // Check for empty response — Ollama sometimes returns valid stream
                    // with no actual content
                    if resp.assistant_message.content.is_empty() {
                        handler.on_error("Ollama stream returned no content, falling back to non-streaming");
                        return self
                            .send_message_non_stream(system, messages, tool_schemas, handler)
                            .await;
                    }
                    return Ok(resp);
                }
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(e)) => {
                    handler.on_error(&format!("Ollama streaming failed, falling back to non-streaming: {}", e));
                    return self
                        .send_message_non_stream(system, messages, tool_schemas, handler)
                        .await;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_error.unwrap_or(crate::LlmError::Network(
            "Ollama: max retries exceeded".to_string(),
        )))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }
}
