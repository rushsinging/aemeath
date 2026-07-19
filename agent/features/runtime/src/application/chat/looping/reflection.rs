//! Shared reflection orchestration used by both TUI and REPL paths.

use std::sync::Arc;

use crate::application::reflection::{
    run_complete_reflection, ReflectionRunMode, ReflectionTaskAdapter, ReflectionTaskRequest,
    ReflectionTaskSubmitOutcome, ReflectionTaskTrigger,
};
use crate::ports::ProviderBinding;
use memory::api::{MemoryPort, ReflectionEngine, ReflectionHistoryStore, ReflectionPromptPort};

use provider::ProviderStopReason;

/// Legacy/manual PL runner retained for internal compatibility. Automatic
/// triggers use the non-blocking submit functions below.
#[allow(clippy::too_many_arguments)]
pub async fn run_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    binding: &ProviderBinding,
    system_prompt_text: &str,
    lang: &str,
    memory: &dyn MemoryPort,
    reflection: &dyn ReflectionPromptPort,
) -> Option<String> {
    match run_complete_reflection(
        ReflectionRunMode::Interval { turn_count },
        config,
        messages,
        binding.provider.as_ref(),
        &binding.model,
        binding.max_tokens,
        binding.requested_reasoning,
        system_prompt_text,
        lang,
        memory,
        reflection,
    )
    .await
    {
        Ok(Some(result)) => Some(result.formatted_content),
        Ok(None) | Err(_) => None,
    }
}

/// Submit interval reflection with an owned message snapshot. This function does
/// not await execution and never exposes generated reflection text to chat UI.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_interval_reflection(
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    turn_count: usize,
    messages: &[share::message::Message],
    binding: &Arc<ProviderBinding>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
) -> ReflectionTaskSubmitOutcome {
    submit(
        adapter,
        ReflectionTaskTrigger::Interval { turn_count },
        config,
        messages.to_vec(),
        binding,
        system_prompt_text,
        lang,
        memory,
        history,
    )
}

#[allow(clippy::too_many_arguments)]
fn submit(
    adapter: &ReflectionTaskAdapter,
    trigger: ReflectionTaskTrigger,
    config: &share::config::MemoryConfig,
    messages: Vec<share::message::Message>,
    binding: &Arc<ProviderBinding>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
) -> ReflectionTaskSubmitOutcome {
    adapter.submit_complete(
        ReflectionTaskRequest::new(trigger, messages),
        config.clone(),
        Arc::clone(&binding.provider),
        binding.model.clone(),
        binding.max_tokens,
        binding.requested_reasoning,
        system_prompt_text.to_owned(),
        lang.to_owned(),
        Arc::clone(memory),
        Arc::new(REFLECTION_ENGINE) as Arc<dyn ReflectionPromptPort>,
        Arc::clone(history),
    )
}

pub(crate) fn should_run_turn_reflection(
    config: &share::config::MemoryConfig,
    turn_count: usize,
    has_tool_calls: bool,
    stop_reason: &ProviderStopReason,
    before_finish_gate_continue: bool,
) -> bool {
    if before_finish_gate_continue
        || !config.enabled
        || !config.reflection.enabled
        || config.reflection.interval_turns == 0
    {
        return false;
    }
    if has_tool_calls && stop_reason != &ProviderStopReason::EndTurn {
        return false;
    }
    turn_count.is_multiple_of(config.reflection.interval_turns)
}

/// Production prompt implementation; exposed here to keep call sites explicit about the port.
pub(crate) const REFLECTION_ENGINE: ReflectionEngine = ReflectionEngine;
