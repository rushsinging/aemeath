//! Shared adapter-level reflection orchestration used by Interval, PreCompact, and
//! Manual Run paths. The three triggers only share this adapter orchestration
//! (`run` → `ReflectionTaskAdapter`); the state machine and `Reflection` activity
//! stay with each owning engine/port path.

use std::sync::Arc;

use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, RuntimeStreamEvent,
};
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskCompletionStatus,
    ReflectionTaskRequest, ReflectionTaskTrigger,
};
use crate::ports::ProviderBindingData;
use memory::api::{MemoryPort, ReflectionHistoryStore};

/// `/reflect-now` 终态的用户可见回执：文案与错误样式成对产出，调用方不得
/// 分开推断两者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManualReflectionNotice {
    pub text: String,
    pub is_error: bool,
}

/// Manual Reflection 终态到 `(文案, is_error)` 的唯一生产策略。六种终态
/// （DisabledSkipped、Succeeded 有/无变更、Failed、Cancelled、TimedOut）在此
/// 一次性映射，保证文案与错误标志不会分叉。
pub(crate) fn manual_outcome_notice(outcome: &ReflectionRunOutcome) -> ManualReflectionNotice {
    let (text, is_error) = match outcome {
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
            match completion.status {
                ReflectionTaskCompletionStatus::Succeeded if changed > 0 => (
                    format!("Reflection 已完成：更新 {changed} 条记忆；摘要可用 /reflect 查询。"),
                    false,
                ),
                ReflectionTaskCompletionStatus::Succeeded => {
                    ("Reflection 已完成：没有记忆变更。".to_string(), false)
                }
                ReflectionTaskCompletionStatus::Cancelled => {
                    ("Reflection 已取消。".to_string(), false)
                }
                ReflectionTaskCompletionStatus::TimedOut => {
                    ("Reflection 超时，已中止。".to_string(), false)
                }
                ReflectionTaskCompletionStatus::Failed => {
                    ("Reflection 执行失败；详情见日志。".to_string(), true)
                }
            }
        }
    };
    ManualReflectionNotice { text, is_error }
}

/// `/reflect-now` 受理结果的用户可见文案。返回 `(text, is_error)`；两者来自
/// 同一条 `manual_outcome_notice` 策略。
pub(crate) fn manual_reflection_outcome_text(outcome: &ReflectionRunOutcome) -> (String, bool) {
    let ManualReflectionNotice { text, is_error } = manual_outcome_notice(outcome);
    (text, is_error)
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

/// 三触发（Interval/PreCompact/Manual）共享的底层 adapter 编排：只执行一次反思任务，
/// 不驱动状态机也不发布 activity——状态机与 `Reflection` activity 由各自 owner 负责：
/// Interval/PreCompact 由 engine 的 reflection phase 经 `ReflectionPhasePort::run_reflection`
/// 驱动，Manual 由 `engine/manual_reflection.rs` 经 `ManualReflectionPort` 驱动。
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
    coverage_end: Option<u64>,
    cancel: tokio_util::sync::CancellationToken,
) -> ReflectionRunOutcome {
    let mut request = ReflectionTaskRequest::new(trigger, messages);
    request.coverage_end = coverage_end;
    adapter
        .run_complete(
            request,
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

/// 反思配置门禁（等价 `run_complete` 内 `ReflectionDisabledReason::of` 的布尔
/// 判定；reason 标签记录仍留在 `run_complete` 内）：memory 关、reflection 关或
/// `interval_runs == 0` 都视为禁用。
pub(crate) fn reflection_enabled(config: &share::config::MemoryConfig) -> bool {
    config.enabled && config.reflection.enabled && config.reflection.interval_runs > 0
}

/// Interval 频控判定：配置开启且用户回合序号（run_ordinal）命中
/// interval 时才反思。
///
/// 唯一生产调用点是 engine 的 Interval 插入点（`ModelStep::Complete` 路径，
/// 必然无未完成工具轮），因此 has_tool_calls/stop_reason/before_finish_gate
/// 三个历史参数已随执行点上移 engine 而删除——它们在该路径上从不参与判定。
pub(crate) fn should_run_turn_reflection(
    config: &share::config::MemoryConfig,
    run_ordinal: usize,
) -> bool {
    if !reflection_enabled(config) {
        return false;
    }
    run_ordinal.is_multiple_of(config.reflection.interval_runs)
}

/// PreCompact 反思材料的共享槽：compaction observer 在 `CompactOutcome::Committed`
/// 时暂存将被压缩丢弃的早期消息，engine 的 reflection phase 在 Compacting 内经
/// 反思端口取出执行。observer 回调拿不到 `&mut Run`，材料收集与状态机经本槽分离。
#[derive(Clone, Default)]
pub(crate) struct PreCompactMaterialSlot(
    std::sync::Arc<std::sync::Mutex<Option<Vec<share::message::Message>>>>,
);

impl PreCompactMaterialSlot {
    /// 暂存材料。仅 `Committed` 调用；`Skipped` 不动槽位。
    pub(crate) fn stage(&self, messages: Vec<share::message::Message>) {
        *self.0.lock().expect("pre-compact 材料槽锁中毒") = Some(messages);
    }

    /// 反思端口取出材料：反思配置关闭时丢弃暂存材料并返回 None——材料既不滞留到
    /// 下次 compact，也不让 engine 空走一次 Reflecting 往返；开启时取走材料返回。
    pub(crate) fn take_for_reflection(
        &self,
        config: &share::config::MemoryConfig,
    ) -> Option<Vec<share::message::Message>> {
        let staged = self.0.lock().expect("pre-compact 材料槽锁中毒").take();
        if reflection_enabled(config) {
            staged
        } else {
            None
        }
    }

    /// 观察当前暂存材料（不清空）。
    #[cfg(test)]
    pub(crate) fn staged(&self) -> Option<Vec<share::message::Message>> {
        self.0.lock().expect("pre-compact 材料槽锁中毒").clone()
    }
}

// ── 反思游标（#1827）─────────────────────────────────────────────

/// 游标增量切片：cursor 有效（≤ 历史长度）→ `历史[cursor..]`（可为空切片）；
/// 缺失（None）或失效（cursor > len，如 compact 截断后）→ None，调用方回退。
pub(crate) fn slice_increment_since_cursor(
    messages: &[share::message::Message],
    cursor: Option<u64>,
) -> Option<Vec<share::message::Message>> {
    let cursor = cursor? as usize;
    if cursor > messages.len() {
        return None;
    }
    Some(messages[cursor..].to_vec())
}

/// 从 history 读最新游标：newest-first 列表中第一条 `Succeeded` 且带
/// `coverage_end` 的记录的游标值。读取失败/无记录/无游标 → None（回退路径）。
pub(crate) async fn latest_coverage_cursor(
    history: &Arc<dyn memory::api::ReflectionHistoryStore>,
) -> Option<u64> {
    let records = history.list(usize::MAX).await.ok()?;
    records
        .iter()
        .find(|record| {
            record.status == memory::api::reflection::ReflectionStatus::Succeeded
                && record.coverage_end.is_some()
        })
        .and_then(|record| record.coverage_end)
}

/// Interval 反思材料：历史增量切片 + 装槽时的历史总长（游标推进基准）。
pub(crate) struct IntervalReflectionMaterial {
    pub messages: Vec<share::message::Message>,
    pub history_len_at_stage: u64,
}

/// Interval 反思材料的共享槽：session driver 在 Main Run 启动时读游标、切片
/// session 历史增量装入；engine 的 reflection phase 经反思端口取出（与 Run 内
/// 增量消息拼接）。与 `PreCompactMaterialSlot` 同构——装配层与状态机分离。
#[derive(Clone, Default)]
pub(crate) struct IntervalReflectionMaterialSlot(
    std::sync::Arc<std::sync::Mutex<Option<IntervalReflectionMaterial>>>,
);

impl IntervalReflectionMaterialSlot {
    pub(crate) fn stage(&self, messages: Vec<share::message::Message>, history_len: u64) {
        *self.0.lock().expect("interval 反思材料槽锁中毒") = Some(IntervalReflectionMaterial {
            messages,
            history_len_at_stage: history_len,
        });
    }

    pub(crate) fn take(&self) -> Option<IntervalReflectionMaterial> {
        self.0.lock().expect("interval 反思材料槽锁中毒").take()
    }
}
