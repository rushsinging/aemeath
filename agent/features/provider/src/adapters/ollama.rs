//! Ollama provider implementation — 主模块
//! 本地 Ollama 推理服务优化：更长超时、可选认证、无 stream_options、空响应检测。

use super::constants::STREAM_IDLE_TIMEOUT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
};
use crate::ports::LlmProvider;

mod conversion;
pub(crate) mod stream;

use conversion::OllamaProviderConversion;

pub struct OllamaProvider {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) user_agent: String,
    pub(crate) http: reqwest::Client,
    pub(crate) timeout_secs: u64,
}

impl OllamaProvider {
    /// `max_tokens` / `reasoning` 不再作为可变运行时状态保留：每次调用的实际
    /// max_tokens / 推理档位由调用方传入的 `InvocationScopeData` 决定（不可变、
    /// 一次调用一份快照）。这两个构造参数仅为保持调用方签名兼容而保留，
    /// 当前未参与任何 immutable default 的派生，故有意不使用。
    #[allow(dead_code)]
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        timeout_secs: u64,
    ) -> Self {
        Self::new_with_user_agent(
            api_key,
            base_url,
            model,
            max_tokens,
            reasoning,
            timeout_secs,
            share::config::Config::default().api.user_agent,
        )
    }

    pub fn new_with_user_agent(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        _max_tokens: u32,
        _reasoning: bool,
        timeout_secs: u64,
        user_agent: String,
    ) -> Self {
        let base_url = base_url.expect("Provider construction 必须传入已解析 base URL");
        let model = model.expect("Provider construction 必须传入已解析模型");
        let http = crate::adapters::transport::build_http_client_for_endpoint(Some(&base_url));
        Self::from_shared_http(
            api_key,
            Some(base_url),
            Some(model),
            timeout_secs,
            user_agent,
            http,
        )
    }

    /// 基于共享（pool 复用的）HTTP client 构造 driver；连接事实由
    /// `ProviderTransport` 持有，本结构只保存引用与展示字段。
    pub(crate) fn from_shared_http(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        timeout_secs: u64,
        user_agent: String,
        http: reqwest::Client,
    ) -> Self {
        Self {
            base_url: {
                let url = base_url.expect("Provider construction 必须传入已解析 base URL");
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            },
            model: model.expect("Provider construction 必须传入已解析模型"),
            api_key,
            user_agent,
            http,
            timeout_secs,
        }
    }

    #[allow(dead_code)]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.http =
            crate::adapters::transport::build_http_client_for_endpoint(Some(&self.base_url));
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

fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    failure.into_provider_error()
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn invocation_stream(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        system: &str,
        _static_prefix_len: usize,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::ProviderResponseStream, crate::ProviderError> {
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let request_body = self
            .build_request_body(resolved, system, messages, tool_schemas, true)
            .map_err(<crate::ProviderError as From<crate::LlmError>>::from)?;
        let url = format!("{}/api/chat", self.base_url);
        let request_bytes = serde_json::to_string(&request_body)
            .map(|value| value.len())
            .unwrap_or(0);
        let context = HttpAttemptContext {
            driver: "ollama",
            api: "chat_stream",
            provider: "ollama",
            model: resolved.model.as_str(),
            method: "POST",
            endpoint: &url,
            attempt: 1,
            max_attempts: 1,
            message_count: messages.len(),
            tool_count: tool_schemas.len(),
            request_bytes,
        };
        let response = HttpAttemptExecutor::execute(
            self.http
                .post(&url)
                .headers(
                    self.build_headers()
                        .map_err(<crate::ProviderError as From<crate::LlmError>>::from)?,
                )
                .json(&request_body),
            &context,
            cancel,
        )
        .await
        .map_err(|failure| {
            failure.log(AttemptDisposition::FinalFailure);
            provider_error_from_attempt(failure)
        })?
        .response;
        Ok(crate::adapters::stream::invocation_stream_from_decoder(
            response,
            resolved.effective_reasoning,
            cancel.child_token(),
            crate::adapters::stream::InvocationDecoder::Ollama,
        ))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn max_reasoning_level(&self) -> crate::domain::capability::ReasoningLevel {
        crate::domain::capability::ReasoningLevel::Medium
    }
}

#[cfg(test)]
#[path = "ollama_tests.rs"]
mod tests;
