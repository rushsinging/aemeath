//! Shared reflection orchestration used by both TUI and REPL paths.

use crate::application::reflection::{run_complete_reflection, ReflectionRunMode};
use crate::LOG_TARGET;
use memory::api::{MemoryPort, ReflectionEngine, ReflectionPromptPort};
use provider::StopReason;

/// Build the reflection context, call the provider, and parse the Memory PL result.
#[allow(clippy::too_many_arguments)]
pub async fn run_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
) -> Option<String> {
    run_reflection_mode(
        ReflectionRunMode::Interval { turn_count },
        config,
        messages,
        client,
        system_prompt_text,
        lang,
        memory,
        reflection,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_precompact_reflection(
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
) -> Option<String> {
    let compacted_messages = context::compact::messages_selected_for_precompact_memory(messages);
    if compacted_messages.is_empty() {
        return None;
    }
    run_reflection_mode(
        ReflectionRunMode::Forced,
        config,
        &compacted_messages,
        client,
        system_prompt_text,
        lang,
        memory,
        reflection,
    )
    .await
}

pub(crate) fn should_run_turn_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    has_tool_calls: bool,
    stop_reason: &StopReason,
    before_finish_gate_continue: bool,
) -> bool {
    if before_finish_gate_continue
        || !config.enabled
        || !config.reflection.enabled
        || config.reflection.interval_turns == 0
    {
        return false;
    }
    if has_tool_calls && stop_reason != &StopReason::EndTurn {
        return false;
    }
    turn_count.is_multiple_of(config.reflection.interval_turns)
}

#[allow(clippy::too_many_arguments)]
async fn run_reflection_mode(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
) -> Option<String> {
    match run_complete_reflection(
        mode,
        config,
        messages,
        client,
        system_prompt_text,
        lang,
        memory,
        reflection,
    )
    .await
    {
        Ok(Some(result)) => Some(result.formatted_content),
        Ok(None) => None,
        Err(error) => {
            log::warn!(target: LOG_TARGET, "Reflection failed: {error}");
            None
        }
    }
}

/// Production prompt implementation; exposed here to keep call sites explicit about the port.
pub(crate) const REFLECTION_ENGINE: ReflectionEngine = ReflectionEngine;
