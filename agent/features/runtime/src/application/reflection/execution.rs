use crate::ports::ProviderPort;
use futures::StreamExt;
use memory::api::reflection::{
    ReflectionErrorCategory, ReflectionExecutionIdentity, ReflectionExecutionResult,
    ReflectionTokenUsage, ReflectionWorkflow, ReflectionWorkflowError,
};
use memory::api::{MemoryPort, ReflectionHistoryStore};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CompleteReflectionResult {
    pub output: memory::api::reflection::ReflectionOutput,
    pub apply_result: Option<memory::api::reflection::ReflectionApplyResult>,
    pub error_category: Option<ReflectionErrorCategory>,
    pub record_id: Option<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ReflectionExecutionError {
    #[error("reflection LLM call failed")]
    LlmCall,
    #[error("reflection LLM returned an empty response")]
    EmptyResponse,
    #[error("reflection response could not be parsed")]
    Unparseable,
    #[error("reflection response contains an invalid suggestion")]
    InvalidSuggestion,
    #[error("reflection history write failed")]
    HistoryWrite,
}

pub type ReflectionExecutionResultType<T> = Result<T, ReflectionExecutionError>;

impl ReflectionExecutionError {
    pub(crate) fn category(self) -> ReflectionErrorCategory {
        match self {
            Self::LlmCall => ReflectionErrorCategory::LlmCall,
            Self::EmptyResponse => ReflectionErrorCategory::EmptyResponse,
            Self::Unparseable => ReflectionErrorCategory::Parse,
            Self::InvalidSuggestion => ReflectionErrorCategory::InvalidSuggestion,
            Self::HistoryWrite => ReflectionErrorCategory::History,
        }
    }
}

impl From<ReflectionWorkflowError> for ReflectionExecutionError {
    fn from(error: ReflectionWorkflowError) -> Self {
        match error {
            ReflectionWorkflowError::Unparseable => Self::Unparseable,
            ReflectionWorkflowError::InvalidSuggestion => Self::InvalidSuggestion,
            ReflectionWorkflowError::HistoryWrite => Self::HistoryWrite,
        }
    }
}

pub(crate) struct ReflectionInvocation<'a> {
    pub provider: &'a dyn ProviderPort,
    pub model: &'a provider::ModelInfo,
    pub max_tokens: u32,
    pub requested_reasoning: share::reasoning::ReasoningLevel,
    pub system_prompt_text: &'a str,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_reflection(
    messages: &[share::message::Message],
    lang: &str,
    auto_apply: bool,
    invocation: ReflectionInvocation<'_>,
    memory: &dyn MemoryPort,
    history: &dyn ReflectionHistoryStore,
    identity: &ReflectionExecutionIdentity,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<CompleteReflectionResult> {
    let started = std::time::Instant::now();
    let prompt = ReflectionWorkflow::build_prompt(messages, lang, memory, identity.timestamp);
    let response = call_provider(&invocation, &prompt, cancel).await;
    let (raw_response, input_tokens, output_tokens) = match response {
        Ok(response) => response,
        Err(error) => {
            ReflectionWorkflow::record_failure(
                history,
                identity,
                error.category(),
                started.elapsed().as_millis() as u64,
            )
            .await?;
            return Err(error);
        }
    };
    let completed: ReflectionExecutionResult = ReflectionWorkflow::complete(
        history,
        memory,
        identity,
        &raw_response,
        lang,
        auto_apply,
        ReflectionTokenUsage {
            input_tokens,
            output_tokens,
        },
        started.elapsed().as_millis() as u64,
    )
    .await?;
    Ok(CompleteReflectionResult {
        output: completed.output,
        apply_result: completed.apply_result,
        error_category: completed.error_category,
        record_id: Some(completed.record_id),
        input_tokens,
        output_tokens,
    })
}

async fn call_provider(
    invocation: &ReflectionInvocation<'_>,
    prompt: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> ReflectionExecutionResultType<(String, u32, u32)> {
    use crate::ports::provider_port::{InvocationRequestData, RequestSystemBlockData};

    let request = InvocationRequestData {
        model: invocation.model.model.clone(),
        cancellation: cancel.clone(),
        messages: vec![share::message::Message::user(prompt)].into(),
        system: vec![RequestSystemBlockData::Text(
            invocation.system_prompt_text.to_string(),
        )],
        tools: vec![],
        max_output_tokens: invocation.max_tokens,
        reasoning: invocation.requested_reasoning,
    };
    let mut stream = invocation
        .provider
        .invoke(request, cancel)
        .await
        .map_err(|_| ReflectionExecutionError::LlmCall)?;
    // 流式端口 → 本地聚合到终止帧（非流式语义经 Stop 帧获得）。
    let mut text = String::new();
    let mut usage = provider::TokenUsageData::default();
    while let Some(chunk) = stream.next().await {
        match chunk {
            provider::ProviderResponseChunk::Content(provider::ProviderContentData::Text(part)) => {
                text.push_str(&part);
            }
            provider::ProviderResponseChunk::Content(_) => {}
            provider::ProviderResponseChunk::Usage(reported) => {
                usage.merge_reported(reported);
            }
            provider::ProviderResponseChunk::Stop(_) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return Err(ReflectionExecutionError::EmptyResponse);
                }
                return Ok((
                    text,
                    usage.input_tokens.unwrap_or(0),
                    usage.output_tokens.unwrap_or(0),
                ));
            }
            provider::ProviderResponseChunk::Error(_) => {
                return Err(ReflectionExecutionError::LlmCall);
            }
        }
    }
    Err(ReflectionExecutionError::LlmCall)
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;

#[cfg(test)]
mod error_boundary_tests {
    use super::ReflectionExecutionError as ReflectionError;

    #[test]
    fn provider_failures_have_stable_display() {
        assert_eq!(
            ReflectionError::LlmCall.to_string(),
            "reflection LLM call failed"
        );
    }
}
