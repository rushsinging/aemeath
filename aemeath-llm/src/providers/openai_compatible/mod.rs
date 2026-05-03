//! OpenAI 兼容 provider 实现
//! 使用 OpenAIProviderConfig 替代旧 Provider enum

mod message_conversion;
mod non_stream;
mod stream;

use aemeath_core::message::Message;
use aemeath_core::provider::ApiDriverKind;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use std::error::Error as StdError;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::client::OpenAIProviderConfig;
use crate::provider::{LlmProvider, StreamHandler};
use crate::types::SystemBlock;

pub(crate) use stream::parse_openai_stream;

#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningConfig {
    Bool(bool),
    Object(serde_json::Value),
}

impl ReasoningConfig {
    fn as_effort(&self) -> Option<String> {
        match self {
            Self::Object(value) => value
                .get("effort")
                .or_else(|| value.get("reasoning_effort"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned),
            Self::Bool(_) => None,
        }
    }
}

pub trait ChatApiDriver: Send + Sync {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    );
}

#[derive(Debug)]
pub struct OpenAiDriver;

#[derive(Debug)]
pub struct ZhipuDriver;

#[derive(Debug)]
pub struct LiteLlmDriver;

impl ChatApiDriver for OpenAiDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        if let Some(ReasoningConfig::Object(reasoning)) = reasoning_config {
            request_body["reasoning"] = reasoning.clone();
        }
    }
}

impl ChatApiDriver for ZhipuDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            _ => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});
    }
}

impl ChatApiDriver for LiteLlmDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        if let Some(ReasoningConfig::Object(reasoning)) = reasoning_config {
            request_body["reasoning"] = reasoning.clone();
        }
    }
}

fn driver_for_api(api: ApiDriverKind) -> Box<dyn ChatApiDriver + Send + Sync> {
    match api {
        ApiDriverKind::OpenAI => Box::new(OpenAiDriver),
        ApiDriverKind::Zhipu => Box::new(ZhipuDriver),
        ApiDriverKind::LiteLLM => Box::new(LiteLlmDriver),
        ApiDriverKind::Anthropic => Box::new(OpenAiDriver),
    }
}

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
    reasoning: Arc<std::sync::atomic::AtomicBool>,
    reasoning_config: Arc<Mutex<Option<ReasoningConfig>>>,
    driver: Box<dyn ChatApiDriver + Send + Sync>,
}

impl OpenAICompatibleProvider {
    pub fn new(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
    ) -> Self {
        let driver = driver_for_api(config.api);
        Self {
            base_url: {
                let url = base_url.unwrap_or_else(|| "https://api.openai.com".to_string());
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
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
            reasoning: Arc::new(std::sync::atomic::AtomicBool::new(reasoning)),
            reasoning_config: Arc::new(Mutex::new(reasoning_config)),
            driver,
        }
    }

    pub fn reasoning_handle(&self) -> Arc<std::sync::atomic::AtomicBool> {
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

    pub(crate) fn chat_url(&self) -> String {
        format!("{}{}", self.base_url, self.config.chat_api_suffix)
    }

    pub(crate) fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );

        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );
        Ok(headers)
    }

    pub(crate) fn apply_reasoning_fields(&self, request_body: &mut serde_json::Value) {
        let reasoning_enabled = self.reasoning.load(std::sync::atomic::Ordering::Relaxed);
        if let Ok(guard) = self.reasoning_config.lock() {
            self.driver
                .apply_reasoning_fields(request_body, guard.as_ref(), reasoning_enabled);
        }
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

        self.apply_reasoning_fields(&mut request_body);

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
                        let rc_len = m
                            .get("reasoning_content")
                            .and_then(|r| r.as_str())
                            .map(|s| s.len() as i32)
                            .unwrap_or(-1);
                        let content_null = m.get("content").map(|c| c.is_null()).unwrap_or(false);
                        summary.push_str(&format!(
                            "\n  [{i}] assistant rc_len={rc_len} tc={has_tc} content_null={content_null}"
                        ));
                    }
                    "tool" => {
                        let tcid = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
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
                self.config.source_key,
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
                let delay =
                    std::time::Duration::from_millis((1000 * 2u64.pow(attempt as u32)).min(30_000));
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
                        attempt + 2,
                        self.max_retries
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
                return self
                    .send_message_non_stream(system, messages, tool_schemas, handler)
                    .await;
            }
        }
        Err(last_error.unwrap_or(crate::LlmError::Network("max retries exceeded".to_string())))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.config.source_key
    }

    fn set_reasoning(&self, enabled: bool) {
        self.reasoning
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut guard) = self.reasoning_config.lock() {
            if matches!(*guard, Some(ReasoningConfig::Bool(_))) {
                *guard = Some(ReasoningConfig::Bool(enabled));
            }
        }
    }

    fn is_reasoning(&self) -> bool {
        self.reasoning.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_reasoning_effort(&self, effort: Option<String>) {
        if let Ok(mut guard) = self.reasoning_config.lock() {
            *guard = effort
                .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })));
        }
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.reasoning_config
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(ReasoningConfig::as_effort))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_body() -> serde_json::Value {
        json!({"model":"test-model","messages":[],"max_tokens":10,"stream":true})
    }

    fn assert_no_reasoning_fields(body: &serde_json::Value) {
        assert!(body.get("reasoning").is_none());
        assert!(body.get("thinking").is_none());
        assert!(body.get("enable_thinking").is_none());
    }

    #[test]
    fn openai_object_reasoning_sends_reasoning_only() {
        let config = ReasoningConfig::Object(json!({"effort":"medium"}));
        let mut body = base_body();

        OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

        assert_eq!(body.get("reasoning"), Some(&json!({"effort":"medium"})));
        assert!(body.get("thinking").is_none());
        assert!(body.get("enable_thinking").is_none());
    }

    #[test]
    fn openai_bool_reasoning_sends_no_reasoning_fields() {
        let config = ReasoningConfig::Bool(true);
        let mut body = base_body();

        OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

        assert_no_reasoning_fields(&body);
    }

    #[test]
    fn zhipu_bool_true_sends_enabled_thinking() {
        let config = ReasoningConfig::Bool(true);
        let mut body = base_body();

        ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), true);

        assert_eq!(body.get("thinking"), Some(&json!({"type":"enabled"})));
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn zhipu_bool_false_sends_disabled_thinking() {
        let config = ReasoningConfig::Bool(false);
        let mut body = base_body();

        ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), false);

        assert_eq!(body.get("thinking"), Some(&json!({"type":"disabled"})));
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn litellm_object_reasoning_passes_through_reasoning() {
        let config = ReasoningConfig::Object(json!({"effort":"high"}));
        let mut body = base_body();

        LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

        assert_eq!(body.get("reasoning"), Some(&json!({"effort":"high"})));
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn litellm_bool_reasoning_sends_no_reasoning_fields() {
        let config = ReasoningConfig::Bool(true);
        let mut body = base_body();

        LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

        assert_no_reasoning_fields(&body);
    }

    #[test]
    fn openai_provider_config_from_api_driver_sets_fields() {
        let openai = OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "source-openai");
        assert_eq!(openai.source_key, "source-openai");
        assert_eq!(openai.api, ApiDriverKind::OpenAI);
        assert_eq!(openai.chat_api_suffix, "/v1/chat/completions");

        let zhipu = OpenAIProviderConfig::from_api_driver(ApiDriverKind::Zhipu, "source-zhipu");
        assert_eq!(zhipu.source_key, "source-zhipu");
        assert_eq!(zhipu.api, ApiDriverKind::Zhipu);
        assert_eq!(zhipu.chat_api_suffix, "/chat/completions");
    }
}
