//! Ollama provider implementation — 主模块
//! 本地 Ollama 推理服务优化：更长超时、可选认证、无 stream_options、空响应检测。

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::adapters::error_log::{log_http_error, log_network_error, ErrorLogContext};
use crate::domain::invoke::{StreamResponse, SystemBlock};
use crate::ports::{LlmProvider, StreamHandler};
use crate::LOG_TARGET;

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
    pub(crate) max_tokens: Arc<AtomicU32>,
    /// If false, send `think: false` to disable reasoning mode for models
    /// that support it (qwen3, deepseek-r1, gpt-oss, etc.)
    pub(crate) reasoning: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) reasoning_level: std::sync::Arc<std::sync::atomic::AtomicU8>,
    pub(crate) user_agent: String,
    pub(crate) http: reqwest::Client,
    pub(crate) max_retries: u32,
    pub(crate) timeout_secs: u64,
}

/// Stream idle timeout（单一真相源：`business::OLLAMA_STREAM_IDLE_TIMEOUT_SECS`）
pub(crate) const STREAM_IDLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(crate::OLLAMA_STREAM_IDLE_TIMEOUT_SECS);

impl OllamaProvider {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        timeout_secs: u64,
    ) -> Self {
        Self {
            base_url: {
                let url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            },
            model: model.unwrap_or_else(|| "llama3.2".to_string()),
            api_key,
            max_tokens: Arc::new(AtomicU32::new(max_tokens)),
            reasoning: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(reasoning)),
            reasoning_level: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
                if reasoning { 3 } else { 0 }, // High or Off
            )),
            user_agent: format!("aemeath/{}", share::version()),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(crate::CONNECT_TIMEOUT_SECS))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs,
        }
    }

    // 重试次数 / 超时为可选构建器旋钮，尚无配置来源接线；保留以备后续从
    // ModelRuntimeSettings 注入（refs #85）。
    #[allow(dead_code)]
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

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

    pub(crate) fn current_max_tokens(&self) -> u32 {
        self.max_tokens.load(Ordering::Relaxed)
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

        let body_bytes = serde_json::to_string(&request_body)
            .map(|s| s.len())
            .unwrap_or(0);
        log::debug!(target: LOG_TARGET,
            "[ollama stream] POST {} model={} think={} msgs={} tools={} body_bytes={}",
            url,
            self.model,
            self.reasoning.load(std::sync::atomic::Ordering::Relaxed),
            messages.len(),
            tool_schemas.len(),
            body_bytes,
        );

        let invocation_started = std::time::Instant::now();
        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Cancelled);
            }

            if attempt > 0 {
                let delay =
                    std::time::Duration::from_millis((1000 * 2u64.pow(attempt)).min(30_000));
                log::debug!(target: LOG_TARGET,
                    "[ollama stream] retry {}/{} after {:?}",
                    attempt,
                    self.max_retries,
                    delay
                );
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
                .post(&url)
                .headers(headers.clone())
                .json(&request_body)
                .send();

            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(crate::LlmError::Cancelled);
                }
                result = send_fut => {
                    match result {
                        Ok(r) => r,
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("timed out") || msg.contains("timeout") {
                                let remaining = self.max_retries.saturating_sub(attempt + 1);
                                log_network_error(
                                    ErrorLogContext {
                                        driver: "ollama",
                                        api: "chat_stream",
                                        provider: "ollama",
                                        model: &self.model,
                                        endpoint: &url,
                                        attempt: attempt + 1,
                                        max_attempts: self.max_retries,
                                        elapsed_ms: invocation_started.elapsed().as_millis(),
                                        message_count: messages.len(),
                                        tool_count: tool_schemas.len(),
                                        request_bytes: body_bytes,
                                    },
                                    &e,
                                    remaining > 0,
                                );
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
                            log_network_error(
                                ErrorLogContext {
                                    driver: "ollama",
                                    api: "chat_stream",
                                    provider: "ollama",
                                    model: &self.model,
                                    endpoint: &url,
                                    attempt: attempt + 1,
                                    max_attempts: self.max_retries,
                                    elapsed_ms: invocation_started.elapsed().as_millis(),
                                    message_count: messages.len(),
                                    tool_count: tool_schemas.len(),
                                    request_bytes: body_bytes,
                                },
                                &e,
                                false,
                            );
                            return Err(crate::LlmError::Network(detailed));
                        }
                    }
                }
            };

            let status = response.status();
            log::debug!(target: LOG_TARGET,
                "[ollama stream] attempt={} HTTP {} content-type={:?}",
                attempt,
                status,
                response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
            );

            if status == 429 {
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                log_http_error(
                    ErrorLogContext {
                        driver: "ollama",
                        api: "chat_stream",
                        provider: "ollama",
                        model: &self.model,
                        endpoint: &url,
                        attempt: attempt + 1,
                        max_attempts: self.max_retries,
                        elapsed_ms: invocation_started.elapsed().as_millis(),
                        message_count: messages.len(),
                        tool_count: tool_schemas.len(),
                        request_bytes: body_bytes,
                    },
                    status,
                    &error_body,
                    remaining > 0,
                );
                last_error = Some(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: error_body,
                });
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                log_http_error(
                    ErrorLogContext {
                        driver: "ollama",
                        api: "chat_stream",
                        provider: "ollama",
                        model: &self.model,
                        endpoint: &url,
                        attempt: attempt + 1,
                        max_attempts: self.max_retries,
                        elapsed_ms: invocation_started.elapsed().as_millis(),
                        message_count: messages.len(),
                        tool_count: tool_schemas.len(),
                        request_bytes: body_bytes,
                    },
                    status,
                    &body,
                    false,
                );
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
                        handler.on_error(
                            "Ollama stream returned no content, falling back to non-streaming",
                        );
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
                    handler.on_error(&format!(
                        "Ollama streaming failed, falling back to non-streaming: {}",
                        e
                    ));
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

    fn set_max_tokens(&self, max_tokens: u32) {
        if max_tokens > 0 {
            self.max_tokens.store(max_tokens, Ordering::Relaxed);
        }
    }

    fn max_tokens(&self) -> u32 {
        self.current_max_tokens()
    }

    fn set_reasoning_level(&self, level: crate::ports::ReasoningLevel) {
        // Ollama 仅支持 thinking 开关，无档位概念
        let enabled = !matches!(level, crate::ports::ReasoningLevel::Off);
        self.reasoning
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        self.reasoning_level
            .store(level.as_u8(), std::sync::atomic::Ordering::Relaxed);
    }

    fn current_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        crate::ports::ReasoningLevel::from_u8(
            self.reasoning_level
                .load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    fn max_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        crate::ports::ReasoningLevel::Medium
    }
}
