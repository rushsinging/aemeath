use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
};
use crate::domain::invoke::{InvocationScope, SystemBlock};
use crate::ports::{LlmProvider, ReasoningLevel};

use super::{OpenAICompatibleProvider, ReasoningConfig};

impl OpenAICompatibleProvider {
    pub(crate) async fn invoke_single_request_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let (request_body, url, api, decoder) = if self.config.use_responses_api {
            (
                self.build_responses_request_body(scope, system, messages, tool_schemas, true),
                self.responses_url(),
                "responses_stream",
                crate::adapters::stream::InvocationDecoder::OpenAiResponses,
            )
        } else {
            let openai_messages = Self::convert_messages(
                system,
                messages,
                !matches!(scope.effective_reasoning(), ReasoningLevel::Off),
            )
            .map_err(provider_error_from_llm)?;
            let tools = Self::convert_tools(tool_schemas);
            let mut body = self.base_request_body(scope, openai_messages, true);
            self.apply_reasoning_fields(&mut body, scope);
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
        let context = HttpAttemptContext {
            driver: "openai_compatible",
            api,
            provider: &self.config.source_key,
            model: scope.model(),
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
                .headers(self.build_headers().map_err(provider_error_from_llm)?)
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
            scope.effective_reasoning(),
            cancel.child_token(),
            decoder,
        ))
    }

    pub(crate) fn base_request_body(
        &self,
        scope: &InvocationScope,
        messages: Vec<serde_json::Value>,
        stream: bool,
    ) -> serde_json::Value {
        let max_tokens_field = self.driver.max_tokens_field();
        let mut request_body = serde_json::json!({
            "model": scope.model(),
            "messages": messages,
            max_tokens_field: scope.max_tokens(),
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
        scope: &InvocationScope,
    ) {
        let reasoning_enabled = !matches!(scope.effective_reasoning(), ReasoningLevel::Off);
        let scoped_config = self
            .reasoning_config
            .as_ref()
            .map(|config| config.for_scope(scope.effective_reasoning(), self.driver.as_ref()))
            .unwrap_or_else(|| {
                ReasoningConfig::from_scope(scope.effective_reasoning(), self.driver.as_ref())
            });
        self.driver
            .apply_reasoning_fields(request_body, Some(&scoped_config), reasoning_enabled);
    }
}

fn provider_error_from_llm(error: crate::LlmError) -> crate::ProviderError {
    let kind = match error {
        crate::LlmError::Cancelled => crate::ProviderErrorKind::Cancelled,
        crate::LlmError::RateLimited => crate::ProviderErrorKind::RateLimited,
        crate::LlmError::ContextTooLong => crate::ProviderErrorKind::ContextTooLong,
        crate::LlmError::Network(_) => crate::ProviderErrorKind::Network,
        crate::LlmError::Api { .. } => crate::ProviderErrorKind::UpstreamUnavailable,
        crate::LlmError::StreamInterrupted(_) | crate::LlmError::StreamTruncated { .. } => {
            crate::ProviderErrorKind::StreamTruncated
        }
        crate::LlmError::Stream(_) => crate::ProviderErrorKind::Protocol,
        crate::LlmError::Config(_) => crate::ProviderErrorKind::Configuration,
    };
    crate::ProviderError::fatal(kind, error.to_string())
}

fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    failure.into_provider_error()
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn invocation_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        self.invoke_single_request_stream(scope, system, messages, tool_schemas, cancel)
            .await
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.config.source_key
    }

    fn max_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        self.driver.max_reasoning_level()
    }
}
