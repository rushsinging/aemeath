use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
};
use crate::ports::{LlmProvider, ReasoningLevel};

use super::{OpenAICompatibleProvider, ReasoningConfig};

impl OpenAICompatibleProvider {
    pub(crate) async fn invoke_single_request_stream(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        system: &[crate::RequestSystemBlockData],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStreamData, crate::ProviderError> {
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let (request_body, url, api, decoder) = if self.config.use_responses_api {
            (
                self.build_responses_request_body(resolved, system, messages, tool_schemas, true),
                self.responses_url(),
                "responses_stream",
                crate::adapters::stream::InvocationDecoder::OpenAiResponses,
            )
        } else {
            let openai_messages = Self::convert_messages(
                system,
                messages,
                !matches!(resolved.effective_reasoning, ReasoningLevel::Off),
            )
            .map_err(<crate::ProviderError as From<crate::LlmError>>::from)?;
            let tools = Self::convert_tools(tool_schemas);
            let mut body = self.base_request_body(resolved, openai_messages, true);
            self.apply_reasoning_fields(&mut body, resolved);
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(tools);
                body["parallel_tool_calls"] = serde_json::Value::Bool(true);
            }
            (
                body,
                self.chat_url(),
                "chat_completions_stream",
                crate::adapters::stream::InvocationDecoder::OpenAiChat,
            )
        };
        let request_bytes = serde_json::to_string(&request_body)
            .map(|value| value.len())
            .unwrap_or(0);
        log_request_body(api, &url, &request_body, request_bytes);
        let context = HttpAttemptContext {
            driver: "openai_compatible",
            api,
            provider: &self.config.source_key,
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
            decoder,
        ))
    }

    pub(crate) fn base_request_body(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        messages: Vec<serde_json::Value>,
        stream: bool,
    ) -> serde_json::Value {
        let max_tokens_field = self.driver.max_tokens_field();
        let mut request_body = serde_json::json!({
            "model": resolved.model.as_str(),
            "messages": messages,
            max_tokens_field: resolved.max_tokens,
            "stream": stream,
        });

        if stream {
            request_body["stream_options"] = serde_json::json!({ "include_usage": true });
        }

        request_body
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

    pub(crate) fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        resolved: &crate::ports::ResolvedInvocation,
    ) {
        let reasoning_enabled = !matches!(resolved.effective_reasoning, ReasoningLevel::Off);
        let scoped_config = self
            .reasoning_config
            .as_ref()
            .map(|config| config.for_scope(resolved.effective_reasoning, self.driver.as_ref()))
            .unwrap_or_else(|| {
                ReasoningConfig::from_scope(resolved.effective_reasoning, self.driver.as_ref())
            });
        self.driver
            .apply_reasoning_fields(request_body, Some(&scoped_config), reasoning_enabled);
    }
}

fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    failure.into_provider_error()
}

/// 构造 debug 级请求摘要 payload：api、endpoint、序列化字节数、
/// 顶层字段名（`serde_json` 默认字典序，排障取字段名集合）与前 200 字符 preview。
///
/// 终点字段不含任何 header / 凭据——headers 不进入本函数签名。
pub(crate) fn build_request_log_summary(
    api: &str,
    endpoint: &str,
    request_bytes: usize,
    body: &serde_json::Value,
) -> serde_json::Value {
    let top_level_keys: Vec<&str> = body
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let body_text = body.to_string();
    let preview: String = body_text.chars().take(200).collect();
    serde_json::json!({
        "event_type": "llm_request",
        "api": api,
        "endpoint": endpoint,
        "request_bytes": request_bytes,
        "top_level_keys": top_level_keys,
        "preview": preview,
    })
}

/// 构造 trace 级完整 wire body payload（排障开关下可复现完整请求）。
///
/// 终点字段不含任何 header / 凭据——headers 不进入本函数签名。
pub(crate) fn build_request_body_log(
    api: &str,
    endpoint: &str,
    body: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "event_type": "llm_request_body",
        "api": api,
        "endpoint": endpoint,
        "body": body,
    })
}

/// 记录实际发出的 LLM 请求：debug 摘要（3.15.4.7.1「请求摘要已截断」）+
/// trace 完整 body（3.15.5 TRACE 行「可记录完整 JSON」，默认关闭）。
fn log_request_body(api: &str, endpoint: &str, body: &serde_json::Value, request_bytes: usize) {
    let summary = build_request_log_summary(api, endpoint, request_bytes, body);
    log::debug!(
        target: crate::LOG_TARGET,
        "{}",
        serde_json::to_string(&summary).unwrap_or_default()
    );
    let full = build_request_body_log(api, endpoint, body);
    log::trace!(
        target: crate::LOG_TARGET,
        "{}",
        serde_json::to_string(&full).unwrap_or_default()
    );
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn invocation_stream(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        system: &[crate::RequestSystemBlockData],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStreamData, crate::ProviderError> {
        self.invoke_single_request_stream(resolved, system, messages, tool_schemas, cancel)
            .await
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.config.source_key
    }

    fn max_reasoning_level(&self) -> crate::domain::capability::ReasoningLevel {
        self.driver.max_reasoning_level()
    }
}
