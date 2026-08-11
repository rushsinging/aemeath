//! Shared reflection orchestration used by both TUI and REPL paths.

use std::sync::Arc;

use crate::application::reflection::{
    ReflectionTaskAdapter, ReflectionTaskRequest, ReflectionTaskSubmitOutcome,
    ReflectionTaskTrigger,
};
use crate::ports::{CompactOutcome, ProviderBinding};
use memory::api::{MemoryPort, ReflectionHistoryStore};

use provider::ProviderStopReason;

/// Submit interval reflection with an owned message snapshot. This function does
/// not await execution and never exposes generated reflection text to chat UI.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_interval_reflection(
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    step_count: usize,
    messages: &[share::message::Message],
    binding: &Arc<ProviderBinding>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
) -> ReflectionTaskSubmitOutcome {
    submit(
        adapter,
        ReflectionTaskTrigger::Interval { step_count },
        config,
        messages.to_vec(),
        binding,
        system_prompt_text,
        lang,
        memory,
        history,
    )
}

/// Submit pre-compact reflection with an owned message snapshot. Only the
/// production automatic compact path (engine-driven `NeedsCompaction`) must call
/// this after `CompactOutcome::Committed`; failures or `Skipped` never submit.
/// The function does not await execution and never exposes generated reflection
/// text to chat UI; the slot is shared with `Interval` and `Manual`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_pre_compact_reflection(
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    binding: &Arc<ProviderBinding>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
) -> ReflectionTaskSubmitOutcome {
    submit(
        adapter,
        ReflectionTaskTrigger::PreCompact,
        config,
        messages.to_vec(),
        binding,
        system_prompt_text,
        lang,
        memory,
        history,
    )
}

/// Decide whether to enqueue a PreCompact reflection job based on the compact
/// outcome. Only `CompactOutcome::Committed` calls
/// `submit_pre_compact_reflection`; `Skipped` returns `None` and never claims
/// the shared slot. The caller remains non-blocking in both cases.
#[allow(clippy::too_many_arguments)]
pub(crate) fn maybe_submit_pre_compact_reflection(
    outcome: &CompactOutcome,
    pre_compact_messages: &[share::message::Message],
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    binding: &Arc<ProviderBinding>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
) -> Option<ReflectionTaskSubmitOutcome> {
    match outcome {
        CompactOutcome::Committed(_) => Some(submit_pre_compact_reflection(
            adapter,
            config,
            pre_compact_messages,
            binding,
            system_prompt_text,
            lang,
            memory,
            history,
        )),
        CompactOutcome::Skipped(_) => None,
    }
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
        Arc::clone(history),
    )
}

pub(crate) fn should_run_turn_reflection(
    config: &share::config::MemoryConfig,
    step_count: usize,
    has_tool_calls: bool,
    stop_reason: &ProviderStopReason,
    before_finish_gate_continue: bool,
) -> bool {
    if before_finish_gate_continue
        || !config.enabled
        || !config.reflection.enabled
        || config.reflection.interval_run_steps == 0
    {
        return false;
    }
    if has_tool_calls && stop_reason != &ProviderStopReason::EndTurn {
        return false;
    }
    step_count.is_multiple_of(config.reflection.interval_run_steps)
}
