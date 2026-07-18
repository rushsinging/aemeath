//! Shared reflection utilities used by both TUI and REPL paths.
//!
//! All paths drive reflection through a single bound `memory::MemoryPort`:
//! Main-run callers pass `BoundMainRun::memory`; idle/forced callers acquire the
//! committed memory under `wiring.with_shared`. The runtime never opens
//! `storage::MemoryStore`.

use crate::application::reflection::run_complete_reflection;
use crate::application::reflection::{CompleteReflectionResult, ReflectionRunMode};
use crate::LOG_TARGET;
use memory::MemoryPort;
use provider::StopReason;

/// Build the reflection context (memory + recent messages), call LLM, parse result.
///
/// Returns `Some(formatted_text)` if reflection was triggered and produced output,
/// or `None` if reflection is disabled, not due yet, or failed silently.
pub async fn run_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    memory: &dyn MemoryPort,
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
) -> Option<String> {
    run_complete(
        ReflectionRunMode::Interval { turn_count },
        config,
        messages,
        memory,
        client,
        system_prompt_text,
        lang,
        "Interval reflection failed",
    )
    .await
    .map(|result| result.formatted_content)
}

pub async fn run_precompact_reflection(
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    memory: &dyn MemoryPort,
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
) -> Option<String> {
    let compacted_messages = context::compact::messages_selected_for_precompact_memory(messages);
    if compacted_messages.is_empty() {
        return None;
    }
    run_complete(
        ReflectionRunMode::Forced,
        config,
        &compacted_messages,
        memory,
        client,
        system_prompt_text,
        lang,
        "Forced reflection failed",
    )
    .await
    .map(|result| result.formatted_content)
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
async fn run_complete(
    mode: ReflectionRunMode,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    memory: &dyn MemoryPort,
    client: &provider::LlmClient,
    system_prompt_text: &str,
    lang: &str,
    failure_label: &str,
) -> Option<CompleteReflectionResult> {
    match run_complete_reflection(
        mode,
        config,
        messages,
        memory,
        client,
        system_prompt_text,
        lang,
    )
    .await
    {
        Ok(Some(result)) => Some(result),
        Ok(None) => None,
        Err(e) => {
            log::warn!(target: LOG_TARGET, "{failure_label}: {e}");
            None
        }
    }
}

#[cfg(test)]
#[path = "reflection_tests.rs"]
mod reflection_tests;
