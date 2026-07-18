use crate::LOG_TARGET;
use memory::api::{
    MemoryLayer, MemoryPort, ReflectionMessage, ReflectionOutput, ReflectionPromptPort,
};
use share::i18n::runtime::reflection as t;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflectionRunMode {
    Interval { turn_count: usize },
    Forced,
}

#[derive(Debug, Clone)]
pub struct CompleteReflectionResult {
    pub output: ReflectionOutput,
    pub formatted_content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub auto_applied: bool,
}

#[derive(Debug, Error)]
pub enum ReflectionError {
    #[error("reflection LLM call failed: {0}")]
    LlmCall(String),
    #[error("reflection LLM returned an empty response")]
    EmptyResponse,
    #[error("reflection response could not be parsed: {0}")]
    Unparseable(String),
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

#[allow(clippy::too_many_arguments)]
pub async fn run_complete_reflection(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
) -> ReflectionResult<Option<CompleteReflectionResult>> {
    if !should_run_reflection(mode, config) {
        return Ok(None);
    }

    let entries = memory.list(Some(MemoryLayer::Project));
    let project_memory = reflection.format_memory_summary(&entries);
    let reflection_messages = messages
        .iter()
        .map(|message| {
            let role = match message.role {
                share::message::Role::User => "user",
                share::message::Role::Assistant => "assistant",
            };
            ReflectionMessage::new(role, message.text_content())
        })
        .collect::<Vec<_>>();
    let recent_summary = reflection.recent_messages_summary(&reflection_messages, usize::MAX);
    let prompt = reflection.build_prompt(&project_memory, &recent_summary, lang);

    let (full_response, input_tokens, output_tokens) =
        call_llm_for_reflection(client, &prompt, system_prompt_text).await?;

    let output = reflection.parse_output(&full_response).map_err(|error| {
        ReflectionError::Unparseable(format!(
            "{error}: {}",
            full_response.chars().take(200).collect::<String>()
        ))
    })?;

    let mut formatted_content = reflection.format_output(&output, lang);
    let mut auto_applied = false;
    if config.reflection.auto_apply_suggestions {
        match memory.apply_reflection(&output).await {
            Ok(result) => {
                formatted_content.push_str(&t::auto_apply_summary(
                    lang,
                    result.suggestions_added,
                    result.outdated_marked,
                ));
                auto_applied = true;
            }
            Err(error) => {
                log::warn!(target: LOG_TARGET, "Reflection auto apply failed: {error}");
            }
        }
    }

    Ok(Some(CompleteReflectionResult {
        output,
        formatted_content,
        input_tokens,
        output_tokens,
        auto_applied,
    }))
}

fn should_run_reflection(mode: ReflectionRunMode, config: &share::config::MemoryConfig) -> bool {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return false;
    }
    match mode {
        ReflectionRunMode::Interval { turn_count } => {
            turn_count.is_multiple_of(config.reflection.interval_turns)
        }
        ReflectionRunMode::Forced => true,
    }
}

async fn call_llm_for_reflection(
    client: &provider::LlmClient,
    prompt: &str,
    system_prompt_text: &str,
) -> ReflectionResult<(String, u32, u32)> {
    use futures::StreamExt;
    use provider::SystemBlock;

    let system_blocks = vec![SystemBlock::dynamic(system_prompt_text.to_string())];
    let messages = vec![share::message::Message::user(prompt)];
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut stream = client
        .invocation_stream(
            client.default_scope(),
            &system_blocks,
            &messages,
            &[],
            &cancel,
        )
        .await
        .map_err(|error| ReflectionError::LlmCall(error.to_string()))?;
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
                    return Err(ReflectionError::EmptyResponse);
                }
                let usage = completion.usage.unwrap_or_default();
                return Ok((
                    text,
                    usage.input_tokens.unwrap_or(0),
                    usage.output_tokens.unwrap_or(0),
                ));
            }
            provider::InvocationEvent::Failed(error) => {
                return Err(ReflectionError::LlmCall(error.to_string()));
            }
            provider::InvocationEvent::Delta(_) => {}
        }
    }
    Err(ReflectionError::LlmCall(
        "provider stream ended without terminal event".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::text_completion_stream;
    use async_trait::async_trait;
    use memory::api::{NoOpMemory, ReflectionEngine};
    use provider::{InvocationStream, LlmProvider, ProviderError, SystemBlock};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    struct StaticProvider {
        response: String,
        input_tokens: u32,
        output_tokens: u32,
    }

    #[async_trait]
    impl LlmProvider for StaticProvider {
        async fn invocation_stream(
            &self,
            _scope: &provider::InvocationScope,
            _system: &[SystemBlock],
            _messages: &[share::message::Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> Result<InvocationStream, ProviderError> {
            Ok(text_completion_stream(
                self.response.clone(),
                self.input_tokens,
                self.output_tokens,
            ))
        }

        fn model_name(&self) -> &str {
            "reflection-test-model"
        }

        fn provider_name(&self) -> &str {
            "reflection-test-provider"
        }
    }

    fn client(response: &str) -> provider::LlmClient {
        provider::LlmClient::from_provider(Arc::new(StaticProvider {
            response: response.to_string(),
            input_tokens: 11,
            output_tokens: 22,
        }))
    }

    #[tokio::test]
    async fn disabled_reflection_does_not_call_or_parse_provider() {
        let mut config = share::config::MemoryConfig::default();
        config.reflection.enabled = false;
        let result = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[],
            &client("not json"),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn forced_reflection_uses_memory_pl_and_preserves_usage() {
        let config = share::config::MemoryConfig::default();
        let result = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[share::message::Message::user("reflect")],
            &client(r#"{"deviations":["drift"],"suggested_memories":[]}"#),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.output.deviations, ["drift"]);
        assert_eq!((result.input_tokens, result.output_tokens), (11, 22));
        assert!(result.formatted_content.contains("drift"));
        assert!(!result.auto_applied);
    }

    #[tokio::test]
    async fn auto_apply_is_dispatched_through_memory_port() {
        let mut config = share::config::MemoryConfig::default();
        config.reflection.auto_apply_suggestions = true;
        let result = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[],
            &client(r#"{"suggested_memories":[]}"#),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.auto_applied);
    }

    #[tokio::test]
    async fn malformed_response_is_wrapped_as_local_execution_error() {
        let config = share::config::MemoryConfig::default();
        let error = run_complete_reflection(
            ReflectionRunMode::Forced,
            &config,
            &[],
            &client("not json"),
            "system",
            "en",
            &NoOpMemory,
            &ReflectionEngine,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ReflectionError::Unparseable(_)));
    }
}
