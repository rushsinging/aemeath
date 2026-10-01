//! Anthropic Claude provider implementation

mod message_conversion;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
};
use crate::adapters::stream::parse_invocation_stream;
use crate::domain::invoke::{CreateMessageRequest, SystemBlockData};
use crate::ports::LlmProvider;

use message_conversion::{apply_message_cache_breakpoint, convert_messages, sanitize_tool_schemas};

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    model: String,
    user_agent: String,
    http: reqwest::Client,
    /// Request timeout in seconds — stored for diagnostics; the value is applied
    /// to the reqwest client at construction time.
    #[allow(dead_code)]
    timeout_secs: u64,
}

impl AnthropicProvider {
    #[allow(dead_code)]
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning_level: crate::domain::capability::ReasoningLevel,
        timeout_secs: u64,
    ) -> Self {
        Self::new_with_user_agent(
            api_key,
            base_url,
            model,
            max_tokens,
            reasoning_level,
            timeout_secs,
            share::config::Config::default().api.user_agent,
        )
    }

    pub fn new_with_user_agent(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        _max_tokens: u32,
        _reasoning_level: crate::domain::capability::ReasoningLevel,
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
            api_key,
            base_url: base_url.expect("Provider construction 必须传入已解析 base URL"),
            model: model.expect("Provider construction 必须传入已解析模型"),
            user_agent,
            http,
            timeout_secs,
        }
    }

    /// Set request timeout in seconds (builder 旋钮，当前无外部调用点).
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

    pub(crate) async fn invoke_stream(
        &self,
        scope: &crate::InvocationScopeData,
        system: &[SystemBlockData],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStreamData, crate::ProviderError> {
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let mut api_messages = convert_messages(messages);
        apply_message_cache_breakpoint(&mut api_messages);
        let mut cached_tools = sanitize_tool_schemas(tool_schemas);
        if let Some(last_tool) = cached_tools.last_mut() {
            if let Some(object) = last_tool.as_object_mut() {
                object.insert(
                    "cache_control".to_string(),
                    serde_json::json!({"type": "ephemeral"}),
                );
            }
        }
        let effort = match scope.effective_reasoning() {
            crate::domain::capability::ReasoningLevel::Off => None,
            level => Some(level.as_str().to_string()),
        };
        let request = CreateMessageRequest::new(
            scope.model().to_string(),
            scope.max_tokens(),
            effort,
            system.to_vec(),
            api_messages,
            cached_tools,
            true,
        );
        let request_json = request.into_json();
        let endpoint = format!("{}/v1/messages", self.base_url);
        let request_bytes = serde_json::to_string(&request_json)
            .map(|value| value.len())
            .unwrap_or(0);
        let context = HttpAttemptContext {
            driver: "anthropic",
            api: "messages_stream",
            provider: "anthropic",
            model: scope.model(),
            method: "POST",
            endpoint: &endpoint,
            attempt: 1,
            max_attempts: 1,
            message_count: messages.len(),
            tool_count: tool_schemas.len(),
            request_bytes,
        };
        let response = HttpAttemptExecutor::execute(
            self.http
                .post(&endpoint)
                .headers(
                    self.build_headers()
                        .map_err(<crate::ProviderError as From<crate::LlmError>>::from)?,
                )
                .json(&request_json),
            &context,
            cancel,
        )
        .await
        .map_err(|failure| {
            failure.log(AttemptDisposition::FinalFailure);
            provider_error_from_attempt(failure)
        })?
        .response;
        Ok(parse_invocation_stream(
            response,
            scope.effective_reasoning(),
            cancel.child_token(),
        ))
    }
}

/// Pull-stream failures share the single typed classification maintained by
/// [`HttpAttemptFailure::into_provider_error`], so every driver maps the same
/// `HttpFailureKind` (including the newer `Authentication` / `PermissionDenied`
/// / `ModelUnavailable`) to the same `ProviderErrorKind` and retryability.
/// The caller logs `FinalFailure` before handing the failure here, so this
/// consumes the (already-logged) failure into the port error.
fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    failure.into_provider_error()
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn invocation_stream(
        &self,
        scope: &crate::InvocationScopeData,
        system: &[SystemBlockData],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStreamData, crate::ProviderError> {
        self.invoke_stream(scope, system, messages, tool_schemas, cancel)
            .await
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn max_reasoning_level(&self) -> crate::domain::capability::ReasoningLevel {
        crate::domain::capability::ReasoningLevel::Max
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
