//! Shared reflection utilities used by both TUI and REPL paths.

use crate::application::reflection::runner::run_complete_reflection_with_base_dir;
use crate::application::reflection::ReflectionRunMode;
use crate::LOG_TARGET;
use provider::api::StopReason;
use std::path::{Path, PathBuf};

/// Build the reflection context (memory + recent messages), call LLM, parse result.
///
/// Returns `Some(formatted_text)` if reflection was triggered and produced output,
/// or `None` if reflection is disabled, not due yet, or failed silently.
pub async fn run_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
    lang: &str,
) -> Option<String> {
    run_reflection_with_base_dir(
        config,
        turn_count,
        messages,
        cwd,
        client,
        system_prompt_text,
        storage::memory_base_dir(),
        lang,
    )
    .await
}

pub async fn run_precompact_reflection(
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
    lang: &str,
) -> Option<String> {
    let compacted_messages =
        context::api::compact::messages_selected_for_precompact_memory(messages);
    if compacted_messages.is_empty() {
        return None;
    }
    run_forced_reflection_with_base_dir(
        config,
        &compacted_messages,
        cwd,
        client,
        system_prompt_text,
        storage::memory_base_dir(),
        lang,
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

async fn run_forced_reflection_with_base_dir(
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
    base_dir: PathBuf,
    lang: &str,
) -> Option<String> {
    match run_complete_reflection_with_base_dir(
        ReflectionRunMode::Forced,
        config,
        messages,
        cwd,
        client,
        system_prompt_text,
        base_dir,
        lang,
    )
    .await
    {
        Ok(Some(result)) => Some(result.formatted_content),
        Ok(None) => None,
        Err(e) => {
            log::warn!(target: LOG_TARGET, "Forced reflection failed: {e}");
            None
        }
    }
}
#[allow(clippy::too_many_arguments)]
async fn run_reflection_with_base_dir(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    cwd: &Path,
    client: &provider::api::LlmClient,
    system_prompt_text: &str,
    base_dir: PathBuf,
    lang: &str,
) -> Option<String> {
    match run_complete_reflection_with_base_dir(
        ReflectionRunMode::Interval { turn_count },
        config,
        messages,
        cwd,
        client,
        system_prompt_text,
        base_dir,
        lang,
    )
    .await
    {
        Ok(Some(result)) => Some(result.formatted_content),
        Ok(None) => None,
        Err(e) => {
            log::warn!(target: LOG_TARGET, "Interval reflection failed: {e}");
            None
        }
    }
}

#[cfg(test)]
#[path = "reflection_tests.rs"]
mod reflection_tests;
