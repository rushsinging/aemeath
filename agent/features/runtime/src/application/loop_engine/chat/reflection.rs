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
        ReflectionRunOutcome::BusySkipped => {
            ("已有反思在进行中，已跳过本次触发。".to_string(), false)
        }
        ReflectionRunOutcome::Detached => ("Reflection 已在后台开始。".to_string(), false),
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

/// 三触发（Interval/PreCompact/Manual）共享的底层 adapter 编排：只派发一次反思任务
/// （统一 detached），不驱动状态机也不发布 activity——状态机与 `Reflection` activity
/// 由各自 owner 负责：Interval/PreCompact 由 engine 的 reflection phase 经
/// `ReflectionPhasePort::run_reflection` 驱动，Manual 由 `engine/manual_reflection.rs` 经
/// `ManualReflectionPort` 驱动。`cancel` 不再透传执行（detached 持独立 token，
/// cancel 不影响异步，#2002）；参数保留待上层签名统一退役。
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
    _cancel: tokio_util::sync::CancellationToken,
    completion_hook: Option<crate::application::reflection::DetachedCompletionHook>,
) -> ReflectionRunOutcome {
    let mut request = ReflectionTaskRequest::new(trigger, messages);
    request.coverage_end = coverage_end;
    // 反思专用 binding（配置 `memory.reflection.model`，装配期缓存在 adapter 上）
    // 优先；未配置或解析失败（已 fail-open 记 warn）时回退调用方传入的会话 binding。
    let binding = effective_binding(adapter, binding);
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
            completion_hook,
        )
        .await
}

/// detached 完成点的 usage 记账入参快照（'static，随 hook 捕获）。
#[derive(Clone)]
pub(crate) struct DetachedUsageRecording {
    pub usage_sink: std::sync::Arc<dyn crate::ports::UsageSink>,
    pub model: provider::ModelInfo,
    pub session_id: sdk::SessionId,
    pub run_id: sdk::RunId,
    /// Manual Reflection Run 无 RunStep：None → 仅记账 id。
    pub run_step_id: Option<sdk::RunStepId>,
}

/// 快照版 usage 记账（原同步路径 `record_succeeded_usage` 的唯一继承实现，
/// #2002 M2）：Succeeded 且带 usage metadata 时经共享 `record_successful_usage`。
pub(crate) fn record_succeeded_usage_with(
    recording: &DetachedUsageRecording,
    metadata: &crate::application::reflection::ReflectionTaskMetadata,
) {
    let usage = crate::ports::TokenUsageData {
        input_tokens: Some(metadata.input_tokens),
        output_tokens: Some(metadata.output_tokens),
        ..crate::ports::TokenUsageData::default()
    };
    crate::application::model::invocation::record_successful_usage(
        recording.usage_sink.as_ref(),
        crate::application::model::usage::UsageRecordContext {
            session_id: recording.session_id.clone(),
            run_id: recording.run_id.clone(),
            run_step_id: recording
                .run_step_id
                .clone()
                .unwrap_or_else(sdk::RunStepId::new_v7),
            model_invocation_id: sdk::ModelInvocationId::new_v7(),
            model: recording.model.clone(),
        },
        usage,
        crate::application::model::invocation::unix_timestamp_millis,
    );
}

/// detached 完成点标准副作用（#2002 M2，三触发共用，调用方各自构造快照）：
/// - Succeeded + usage metadata → 记账 usage（快照路径）；
/// - Manual 全终态 → 恰好一次 CommandResultText 回执（成功有/无变更均可感知）；
/// - 自动反思 Succeeded + 有记忆变更 → 通用 memory update notice；
/// - 自动反思失败或成功无变更保持静默（不打扰主对话）；
/// - PreCompact → 推进材料槽游标缓存（统一覆盖去重；写入失败接受重复，D5）。
pub(crate) fn detached_completion_hook(
    sink: ChatEventSinkHandle,
    lang: String,
    usage: Option<DetachedUsageRecording>,
    pre_compact_slot: Option<PreCompactMaterialSlot>,
    coverage_end: Option<u64>,
    trigger: ReflectionTaskTrigger,
) -> crate::application::reflection::DetachedCompletionHook {
    std::sync::Arc::new(move |outcome| {
        // Fn 闭包可重入：先克隆捕获快照（借用），再 move 进一次性 async 块。
        let sink = sink.clone();
        let lang = lang.clone();
        let usage = usage.clone();
        let pre_compact_slot = pre_compact_slot.clone();
        Box::pin(async move {
            if let ReflectionRunOutcome::Completed(completion) = &outcome {
                match completion.status {
                    ReflectionTaskCompletionStatus::Succeeded => {
                        if let (Some(usage), Some(metadata)) = (&usage, &completion.metadata) {
                            record_succeeded_usage_with(usage, metadata);
                        }
                        if matches!(trigger, ReflectionTaskTrigger::PreCompact) {
                            if let Some(slot) = &pre_compact_slot {
                                if let Some(coverage_end) = coverage_end {
                                    slot.advance_cursor_if_farther(coverage_end);
                                }
                            }
                        }
                    }
                    ReflectionTaskCompletionStatus::Cancelled
                    | ReflectionTaskCompletionStatus::Failed
                    | ReflectionTaskCompletionStatus::TimedOut => {}
                }
                if matches!(trigger, ReflectionTaskTrigger::Manual) {
                    let ManualReflectionNotice { text, is_error } = manual_outcome_notice(&outcome);
                    let is_error = is_error
                        || matches!(completion.status, ReflectionTaskCompletionStatus::TimedOut);
                    sink.send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                        .await;
                }
            }
            if !matches!(trigger, ReflectionTaskTrigger::Manual) {
                announce_memory_update(&sink, &outcome, &lang).await;
            }
        })
    })
}

/// 反思 binding 选择：装配期缓存的反思专用 binding（`memory.reflection.model`）
/// 优先；`None`（未配置或解析失败）时返回会话 binding——反思是辅助功能，
/// 永不因模型解析失败阻断主流程（fail-open）。
pub(crate) fn effective_binding<'a>(
    adapter: &'a ReflectionTaskAdapter,
    session_binding: &'a Arc<ProviderBindingData>,
) -> &'a Arc<ProviderBindingData> {
    adapter.reflection_binding().unwrap_or(session_binding)
}

/// 装配期缓存反思专用 binding 的唯一收口：`Ok` 写入 session 级 adapter
/// （随 clone 在三个触发点间共享同一 `Arc`，每个 session 只解析一次 provider）；
/// `Err` 记 warn 并保持 `None`——调用方经 [`effective_binding`] 回退会话 binding。
pub(crate) fn cache_resolved_reflection_binding(
    adapter: &mut ReflectionTaskAdapter,
    selection: &str,
    resolved: Result<Arc<ProviderBindingData>, String>,
) {
    match resolved {
        Ok(binding) => adapter.set_reflection_binding(Some(binding)),
        Err(err) => log::warn!(
            target: crate::LOG_TARGET,
            "[reflection_model_fallback] selection={} err={}；反思回退会话模型",
            selection,
            err
        ),
    }
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
/// 时暂存将被压缩丢弃的早期消息（按最新 `Succeeded` 游标过滤已覆盖段——已 reflect
/// 的 context 不再进入 reflect），engine 的 reflection phase 在 Compacting 内经
/// 反思端口取出执行。observer 回调拿不到 `&mut Run`，材料收集与状态机经本槽分离。
/// BusySkipped 时未派发残余经 `restore` 回填滞留（下次 compact 后再试）。
#[derive(Clone, Default)]
pub(crate) struct PreCompactMaterialSlot(
    std::sync::Arc<std::sync::Mutex<Option<StagedPreCompactMaterial>>>,
);

/// PreCompact 反思材料：按游标过滤后的未覆盖残余 + 游标推进基准
/// （`coverage_end` = 丢弃段在压缩前历史坐标系中的末位置）。
#[derive(Debug, Clone)]
pub(crate) struct PreCompactReflectionMaterial {
    pub messages: Vec<share::message::Message>,
    /// 成功反思后的游标推进基准（压缩前历史坐标）。
    pub coverage_end: Option<u64>,
    /// `messages` 首元素在压缩前历史中的绝对坐标（restore 保持绝对坐标语义）。
    array_origin: u64,
    /// take 时的已知游标快照（restore 语义重建用）。
    known_cursor: Option<u64>,
}

/// PreCompact 槽内部暂存态：丢弃段快照 + 段末坐标 + 已知最新游标。
#[derive(Default)]
struct StagedPreCompactMaterial {
    messages: Vec<share::message::Message>,
    /// 丢弃段末位置（压缩前历史坐标），派发时作为 `coverage_end` 基准。
    segment_end: Option<u64>,
    /// `messages` 首元素的绝对坐标（首次 stage 恒为 0）。
    array_origin: u64,
    /// 已知最新 `Succeeded` 游标（stage 时从 history 读取；restore/advance 只前进）。
    known_cursor: Option<u64>,
}

impl StagedPreCompactMaterial {
    /// 按已知游标过滤出未覆盖残余；返回 `(残余, 是否可派发)`。
    fn uncovered_residual(&self) -> (Vec<share::message::Message>, bool) {
        let Some(cursor) = self.known_cursor else {
            return (self.messages.clone(), !self.messages.is_empty());
        };
        let relative = cursor.saturating_sub(self.array_origin) as usize;
        let residual = self
            .messages
            .get(relative..)
            .map(Vec::from)
            .unwrap_or_default();
        let dispatchable = cursor
            < self
                .segment_end
                .unwrap_or_else(|| self.array_origin + self.messages.len() as u64)
            && !residual.is_empty();
        (residual, dispatchable)
    }
}

impl PreCompactMaterialSlot {
    /// 暂存丢弃段快照并刷新已知游标（observer 在 `CompactOutcome::Committed` 时
    /// 从 history 读取游标传入）。已有回填滞留时覆盖（新丢弃段优先）并记 log。
    pub(crate) fn stage(&self, messages: Vec<share::message::Message>, cursor: Option<u64>) {
        let mut staged = self.0.lock().expect("pre-compact 材料槽锁中毒");
        if staged.is_some() {
            log::warn!(
                target: crate::LOG_TARGET,
                "[pre_compact_material_overwritten] 槽内回填滞留被新丢弃段覆盖，滞留残余未派发",
            );
        }
        *staged = Some(StagedPreCompactMaterial {
            segment_end: Some(messages.len() as u64),
            messages,
            array_origin: 0,
            known_cursor: cursor,
        });
    }

    /// 反思端口取出材料：反思配置关闭时丢弃暂存材料并返回 None——材料既不滞留到
    /// 下次 compact，也不让 engine 空走一次 Reflecting 往返；开启时按已知游标
    /// 过滤出未覆盖残余（整段已覆盖 → 不派发），取走材料返回。
    pub(crate) fn take_for_reflection(
        &self,
        config: &share::config::MemoryConfig,
    ) -> Option<PreCompactReflectionMaterial> {
        let staged = self.0.lock().expect("pre-compact 材料槽锁中毒").take()?;
        if !reflection_enabled(config) {
            return None;
        }
        let segment_end = staged
            .segment_end
            .unwrap_or(staged.array_origin + staged.messages.len() as u64);
        let known_cursor = staged.known_cursor;
        // 残余起点绝对坐标（restore 后保持：相对数组下标 + 数组首元素绝对坐标）。
        let residual_origin = staged.array_origin
            + known_cursor
                .map(|cursor| cursor.saturating_sub(staged.array_origin))
                .unwrap_or_default();
        let (residual, dispatchable) = staged.uncovered_residual();
        dispatchable.then_some(PreCompactReflectionMaterial {
            messages: residual,
            coverage_end: Some(segment_end),
            array_origin: residual_origin,
            known_cursor,
        })
    }

    /// BusySkipped 时回填未派发的残余滞留（下次 compact 后再试）：保持绝对坐标
    /// 语义，已知游标只前进不回退。
    pub(crate) fn restore(&self, material: &PreCompactReflectionMaterial) {
        let mut slot = self.0.lock().expect("pre-compact 材料槽锁中毒");
        let known_cursor = slot
            .as_ref()
            .and_then(|staged| staged.known_cursor)
            .or(material.known_cursor)
            .map(|known| known.max(material.array_origin));
        *slot = Some(StagedPreCompactMaterial {
            segment_end: material.coverage_end,
            messages: material.messages.clone(),
            array_origin: material.array_origin,
            known_cursor,
        });
    }

    /// 已知游标只前进地推进到 `cursor`（反思成功后的游标缓存刷新）。
    pub(crate) fn advance_cursor_if_farther(&self, cursor: u64) {
        let mut slot = self.0.lock().expect("pre-compact 材料槽锁中毒");
        if let Some(staged) = slot.as_mut() {
            staged.known_cursor = Some(
                staged
                    .known_cursor
                    .map_or(cursor, |known| known.max(cursor)),
            );
        }
    }

    /// 观察当前暂存材料（不清空，未过滤）。
    #[cfg(test)]
    pub(crate) fn staged(&self) -> Option<Vec<share::message::Message>> {
        self.0
            .lock()
            .expect("pre-compact 材料槽锁中毒")
            .as_ref()
            .map(|staged| staged.messages.clone())
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

#[cfg(test)]
mod reflection_binding_selection_tests {
    use super::*;
    use crate::application::reflection::test_support::static_reflection_binding;

    fn adapter() -> ReflectionTaskAdapter {
        ReflectionTaskAdapter::production(std::time::Duration::from_secs(240))
    }

    /// 无配置（None）时行为与现状完全一致：反思直接用会话 binding。
    #[test]
    fn unconfigured_adapter_falls_back_to_session_binding() {
        let adapter = adapter();
        let session = static_reflection_binding();

        assert!(Arc::ptr_eq(effective_binding(&adapter, &session), &session));
    }

    /// 解析成功并缓存后，反思优先使用反思专用 binding。
    #[test]
    fn cached_binding_wins_over_session_binding() {
        let mut adapter = adapter();
        let session = static_reflection_binding();
        let reflection_binding = static_reflection_binding();

        cache_resolved_reflection_binding(
            &mut adapter,
            "provider/model",
            Ok(Arc::clone(&reflection_binding)),
        );

        assert!(Arc::ptr_eq(
            effective_binding(&adapter, &session),
            &reflection_binding
        ));
    }

    /// 解析失败 fail-open：保持 None → 回退会话 binding，不阻断反思。
    #[test]
    fn resolution_failure_keeps_session_binding_fallback() {
        let mut adapter = adapter();
        let session = static_reflection_binding();

        cache_resolved_reflection_binding(&mut adapter, "provider/model", Err("boom".into()));

        assert!(adapter.reflection_binding().is_none());
        assert!(Arc::ptr_eq(effective_binding(&adapter, &session), &session));
    }

    /// 缓存随 adapter clone 共享：三个触发点拿到的是同一 session 级 adapter 的
    /// clone，解析一次即可全局生效。
    #[test]
    fn clones_share_the_cached_reflection_binding() {
        let mut adapter = adapter();
        let reflection_binding = static_reflection_binding();
        cache_resolved_reflection_binding(
            &mut adapter,
            "provider/model",
            Ok(Arc::clone(&reflection_binding)),
        );

        let clone = adapter.clone();
        let session = static_reflection_binding();

        assert!(Arc::ptr_eq(
            effective_binding(&clone, &session),
            &reflection_binding
        ));
    }
}
