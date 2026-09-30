//! Shared reflection orchestration used by both TUI and REPL paths.

use std::sync::Arc;

use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, RuntimeStreamEvent,
};
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskCompletionStatus,
    ReflectionTaskRequest, ReflectionTaskTrigger,
};
use crate::ports::{CompactOutcome, ProviderBindingData};
use memory::api::{MemoryPort, ReflectionHistoryStore};

/// Run pre-compact reflection with an owned message snapshot. Only the
/// production automatic compact path (engine-driven `NeedsCompaction`) must call
/// this after `CompactOutcome::Committed`; failures or `Skipped` never run.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_pre_compact_reflection(
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    binding: &Arc<ProviderBindingData>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
    cancel: tokio_util::sync::CancellationToken,
) -> ReflectionRunOutcome {
    run(
        adapter,
        ReflectionTaskTrigger::PreCompact,
        config,
        messages.to_vec(),
        binding,
        system_prompt_text,
        lang,
        memory,
        history,
        cancel,
    )
    .await
}

/// Run manual reflection with an owned message snapshot. Only the
/// `/reflect-now` idle command path calls this after freezing the
/// committed session's visible messages.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_manual_reflection(
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    messages: &[share::message::Message],
    binding: &Arc<ProviderBindingData>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
) -> ReflectionRunOutcome {
    run(
        adapter,
        ReflectionTaskTrigger::Manual,
        config,
        messages.to_vec(),
        binding,
        system_prompt_text,
        lang,
        memory,
        history,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
}

/// `/reflect-now` 受理结果的用户可见文案。返回 `(text, is_error)`。
pub(crate) fn manual_reflection_outcome_text(outcome: &ReflectionRunOutcome) -> (String, bool) {
    match outcome {
        ReflectionRunOutcome::DisabledSkipped => (
            "Memory 或 Reflection 未启用；请在配置中开启后重试。".to_string(),
            false,
        ),
        ReflectionRunOutcome::Completed(completion) => {
            let changed = completion
                .metadata
                .as_ref()
                .map(|metadata| metadata.applied_changes())
                .unwrap_or_default();
            (
                manual_completion_text(completion.status, changed),
                completion.status == ReflectionTaskCompletionStatus::Failed,
            )
        }
    }
}

fn manual_completion_text(status: ReflectionTaskCompletionStatus, changed: usize) -> String {
    match status {
        ReflectionTaskCompletionStatus::Succeeded => {
            if changed > 0 {
                format!("Reflection 已完成：更新 {changed} 条记忆；摘要可用 /reflect 查询。")
            } else {
                "Reflection 已完成：没有记忆变更。".to_string()
            }
        }
        ReflectionTaskCompletionStatus::Cancelled => "Reflection 已取消。".to_string(),
        ReflectionTaskCompletionStatus::TimedOut => "Reflection 超时，已中止。".to_string(),
        ReflectionTaskCompletionStatus::Failed => "Reflection 执行失败；详情见日志。".to_string(),
    }
}

/// TUI notice text for a reflection that changed memory. Shown as soon as the
/// run completes; the LLM-side reminder follows in the next Run.
pub(crate) fn memory_updated_notice_text(changed: usize, lang: &str) -> String {
    if lang == "zh" {
        format!("记忆已更新 {changed} 条")
    } else {
        format!(
            "Memory updated: {changed} entr{}",
            if changed == 1 { "y" } else { "ies" }
        )
    }
}

/// Decide whether to run a PreCompact reflection based on the compact outcome.
/// Only `CompactOutcome::Committed` runs reflection; `Skipped` returns `None`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn maybe_run_pre_compact_reflection(
    outcome: &CompactOutcome,
    pre_compact_messages: &[share::message::Message],
    adapter: &ReflectionTaskAdapter,
    config: &share::config::MemoryConfig,
    binding: &Arc<ProviderBindingData>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
    cancel: tokio_util::sync::CancellationToken,
) -> Option<ReflectionRunOutcome> {
    match outcome {
        CompactOutcome::Committed(_) => Some(
            run_pre_compact_reflection(
                adapter,
                config,
                pre_compact_messages,
                binding,
                system_prompt_text,
                lang,
                memory,
                history,
                cancel,
            )
            .await,
        ),
        CompactOutcome::Skipped(_) => None,
    }
}

/// Send the TUI notice for a completed reflection that changed memory. Returns
/// the number announced, so callers can also report it in the LLM reminder slot.
pub(crate) async fn announce_memory_update(
    sink: &ChatEventSinkHandle,
    outcome: &ReflectionRunOutcome,
    lang: &str,
) -> Option<usize> {
    let ReflectionRunOutcome::Completed(completion) = outcome else {
        return None;
    };
    if completion.status != ReflectionTaskCompletionStatus::Succeeded {
        return None;
    }
    let changed = completion
        .metadata
        .as_ref()
        .map(|metadata| metadata.applied_changes())
        .unwrap_or_default();
    if changed == 0 {
        return None;
    }
    sink.send_event(RuntimeStreamEvent::SystemMessage(
        memory_updated_notice_text(changed, lang),
    ))
    .await;
    Some(changed)
}

/// 三触发共用的反思编排（Interval/PreCompact/Manual），由端口实现
/// （`RuntimeReflection::run_reflection`）复用；触发判定与状态机收口在 engine phase。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(
    adapter: &ReflectionTaskAdapter,
    trigger: ReflectionTaskTrigger,
    config: &share::config::MemoryConfig,
    messages: Vec<share::message::Message>,
    binding: &Arc<ProviderBindingData>,
    system_prompt_text: &str,
    lang: &str,
    memory: &Arc<dyn MemoryPort>,
    history: &Arc<dyn ReflectionHistoryStore>,
    cancel: tokio_util::sync::CancellationToken,
) -> ReflectionRunOutcome {
    adapter
        .run_complete(
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
            cancel,
        )
        .await
}

/// Interval 频控判定：配置开启且 step_count 命中 interval 时才反思。
///
/// 唯一生产调用点是 engine 的 Interval 插入点（`ModelStep::Complete` 路径，
/// 必然无未完成工具轮），因此 has_tool_calls/stop_reason/before_finish_gate
/// 三个历史参数已随执行点上移 engine 而删除——它们在该路径上从不参与判定。
pub(crate) fn should_run_turn_reflection(
    config: &share::config::MemoryConfig,
    step_count: usize,
) -> bool {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_runs == 0 {
        return false;
    }
    step_count.is_multiple_of(config.reflection.interval_runs)
}
