use crate::ports::ProviderPort;
use futures::StreamExt;
use memory::api::{
    MemoryPort, ReflectionErrorCategory, ReflectionExecutionIdentity, ReflectionExecutionResult,
    ReflectionHistoryStore, ReflectionTokenUsage, ReflectionWorkflow, ReflectionWorkflowError,
};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CompleteReflectionResult {
    pub output: memory::api::ReflectionOutput,
    pub apply_result: Option<memory::api::ReflectionApplyResult>,
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
    pub model: &'a provider::ModelId,
    pub max_tokens: u32,
    pub requested_reasoning: provider::ReasoningLevel,
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
    let prompt = ReflectionWorkflow::build_prompt(messages, lang, memory);
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
    use crate::ports::provider_port::{InvocationOptions, InvocationRequest, RequestSystemBlock};

    let request = InvocationRequest {
        model: invocation.model.clone(),
        cancellation: cancel.clone(),
        messages: vec![share::message::Message::user(prompt)].into(),
        system: vec![RequestSystemBlock::Text(
            invocation.system_prompt_text.to_string(),
        )],
        tools: vec![],
        options: InvocationOptions::new(invocation.max_tokens, invocation.requested_reasoning),
    };
    let mut stream = invocation
        .provider
        .invoke(request, cancel)
        .await
        .map_err(|_| ReflectionExecutionError::LlmCall)?;
    while let Some(event) = stream.next().await {
        match event {
            provider::InvocationEvent::Completed(completion) => {
                let text = completion
                    .output
                    .iter()
                    .filter_map(|block| match block {
                        provider::ProviderContentBlock::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>()
                    .trim()
                    .to_string();
                if text.is_empty() {
                    return Err(ReflectionExecutionError::EmptyResponse);
                }
                let usage = completion.usage.unwrap_or_default();
                return Ok((
                    text,
                    usage.input_tokens.unwrap_or(0),
                    usage.output_tokens.unwrap_or(0),
                ));
            }
            provider::InvocationEvent::Failed(_) => {
                return Err(ReflectionExecutionError::LlmCall);
            }
            provider::InvocationEvent::Delta(_) => {}
        }
    }
    Err(ReflectionExecutionError::LlmCall)
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
