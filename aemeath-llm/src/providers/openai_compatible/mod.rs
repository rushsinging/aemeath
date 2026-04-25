//! OpenAI 兼容 provider 实现
//! 支持 OpenAI、OpenRouter、DeepSeek、Moonshot、Zhipu、DashScope 及通用 OpenAI 兼容 API

mod message_conversion;
mod non_stream;
mod stream;

use async_trait::async_trait;
use aemeath_core::message::Message;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use std::error::Error as StdError;
use tokio_util::sync::CancellationToken;

use crate::provider::{LlmProvider, Provider, StreamHandler};
use crate::types::SystemBlock;

pub(crate) use stream::parse_openai_stream;

pub struct OpenAICompatibleProvider {
    provider: Provider,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    user_agent: String,
    http: reqwest::Client,
    /// 最大重试次数（默认 10）
    max_retries: u32,
    /// 请求超时秒数（默认 120）
    timeout_secs: u64,
    /// 是否使用 reasoning/thinking 模式（运行时可切换）
    reasoning: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl OpenAICompatibleProvider {
    pub fn new(
        provider: Provider,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        Self {
            provider,
            base_url: {
                let url = base_url.unwrap_or_else(|| provider.default_base_url().to_string());
                // 去掉末尾的 /v1 以避免构建请求 URL 时出现 /v1/v1
                url.trim_end_matches('/').trim_end_matches("/v1").to_string()
            },
            model: model.unwrap_or_else(|| provider.default_model().to_string()),
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
        }
    }

    /// Get a handle to toggle reasoning at runtime
    pub fn reasoning_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.reasoning.clone()
    }

    /// 设置最大重试次数
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// 设置请求超时秒数
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

        // 不同 provider 使用不同的 header 格式
        match self.provider {
            Provider::OpenRouter => {
                headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| crate::LlmError::Config(e.to_string()))?);
                headers.insert("HTTP-Referer", HeaderValue::from_static("https://github.com/aemeath"));
            }
            _ => {
                headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| crate::LlmError::Config(e.to_string()))?);
            }
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

        // 根据 config 控制 reasoning/thinking 模式
        if !self.reasoning.load(std::sync::atomic::Ordering::Relaxed) {
            request_body["enable_thinking"] = serde_json::json!(false);
        }

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
        }

        // 调试：记录请求体中每条消息的摘要，便于回溯查找导致 400 的具体
        // assistant 消息（例如缺少 reasoning_content 字段）。日志附加到
        // ~/.aemeath/aemeath.log，使用默认 filter（`aemeath_llm=debug`）启用。
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
                "[openai-compat stream] POST provider={:?} body_bytes={} messages={}:{}",
                self.provider,
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
                .post(format!("{}{}", self.base_url, self.provider.chat_api_suffix()))
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
                            let url = format!("{}{}", self.base_url, self.provider.chat_api_suffix());
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
                            // 网络错误可重试 — 向 UI 展示重试进度
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

            // 重试 5xx 错误（服务端问题可能是暂时的）
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
                    // 流解码错误 — 先重试，最后一次尝试时回退到非流式
                    handler.on_error(&format!("Streaming error: {}, retrying...", e));
                    last_error = Some(crate::LlmError::Stream(e));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        // 所有流式重试耗尽 — 尝试最后一次非流式请求
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
        match self.provider {
            Provider::OpenAI => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::DeepSeek => "deepseek",
            Provider::Moonshot => "moonshot",
            Provider::Zhipu => "zhipu",
            Provider::DashScope => "dashscope",
            Provider::MiniMax => "minimax",
            Provider::OpenAICompatible => "openai-compatible",
            Provider::Anthropic => "anthropic", // 不应发生，作为兜底
            Provider::Ollama => "ollama", // 不应发生 — 应使用 OllamaProvider
        }
    }

    fn set_reasoning(&self, enabled: bool) {
        self.reasoning.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_reasoning(&self) -> bool {
        self.reasoning.load(std::sync::atomic::Ordering::Relaxed)
    }
}
