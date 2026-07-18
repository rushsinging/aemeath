use super::apply::apply_output_via_port;
use super::store::project_memory_summary;
use super::types::{ReflectionError, ReflectionResult};
use super::{ReflectionEngine, ReflectionOutput};
use crate::LOG_TARGET;
use memory::MemoryPort;
use share::i18n::runtime::reflection as t;

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

/// Runs a complete reflection cycle against a bound `MemoryPort`.
///
/// The summary is read via `port.list(Project)` and any auto-apply writes back
/// to the same port — the runtime never constructs `storage::MemoryStore`.
#[allow(clippy::too_many_arguments)]
pub async fn run_complete_reflection(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    memory: &dyn MemoryPort,
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
) -> ReflectionResult<Option<CompleteReflectionResult>> {
    if !should_run_reflection(mode, config) {
        return Ok(None);
    }

    let project_memory = project_memory_summary(memory);
    let recent_summary = ReflectionEngine::recent_messages_summary(messages, usize::MAX);
    let prompt = ReflectionEngine::build_prompt(&project_memory, &recent_summary, lang);

    let (full_response, input_tokens, output_tokens) =
        call_llm_for_reflection(client, &prompt, system_prompt_text).await?;

    let output = ReflectionEngine::parse_output(&full_response).map_err(|e| {
        ReflectionError::Unparseable(format!("{e}: {}", truncate_200(&full_response)))
    })?;

    let mut formatted_content = ReflectionEngine::format_output(&output, lang);
    let mut auto_applied = false;
    if config.reflection.auto_apply_suggestions {
        match apply_output_via_port(&output, memory).await {
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

fn truncate_200(s: &str) -> String {
    s.chars().take(200).collect()
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
