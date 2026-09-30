/// 反思执行点上移 engine 后的 L2 协作测试（Interval 与 PreCompact 两触发）。
///
/// 端口只提供判定材料与执行能力：`Reflecting` 状态转移、`Reflection` activity 的
/// 发布/终态收口，以及「任何 outcome 都不终止宿主 Run」的语义全部由 engine 的
/// reflection phase 唯一负责（状态机只在 engine 可达）。装配风格对齐
/// `engine_activity_tests.rs`：production coordinator + fake port。
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskCompletion, ReflectionTaskCompletionStatus,
    ReflectionTaskTrigger,
};
use crate::domain::agent_run::RunTransitionReason;

/// Interval 判定的脚本化结果。生产实现中等价于端口内调用
/// `should_run_turn_reflection`：配置关闭 → `Disabled`；频控未命中 → `None`。
enum IntervalJudgment {
    /// 配置关闭 / 判定不通过：engine 不进入反思 phase（noop）。
    Disabled,
    /// 频控判定（等价 `step_count.is_multiple_of(interval)`），命中时携带消息快照。
    Interval(usize),
}

/// 脚本化反思端口：判定与执行终态全部由测试指定，执行次数与执行期 activity 状态被记录。
struct ReflectionFake {
    judgment: IntervalJudgment,
    outcome: ReflectionRunOutcome,
    activities: std::sync::Arc<crate::application::activity::ActivityCoordinator>,
    runs: Vec<ReflectionTaskTrigger>,
    state_during_run: Option<sdk::ActivityStateView>,
    /// 与压缩侧共享的 PreCompact 材料槽；取材料走生产门禁语义（禁用即丢弃）。
    pre_compact_material:
        crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot,
    memory_config: share::config::MemoryConfig,
}

#[async_trait::async_trait]
impl crate::application::loop_engine::ReflectionPhasePort for ReflectionFake {
    fn interval_reflection_messages(
        &self,
        step_count: usize,
        _messages: &[share::message::Message],
    ) -> Option<Vec<share::message::Message>> {
        match self.judgment {
            IntervalJudgment::Disabled => None,
            IntervalJudgment::Interval(runs) if runs > 0 && step_count.is_multiple_of(runs) => {
                Some(vec![share::message::Message::user("reflect snapshot")])
            }
            IntervalJudgment::Interval(_) => None,
        }
    }

    fn take_pre_compact_messages(&self) -> Option<Vec<share::message::Message>> {
        self.pre_compact_material
            .take_for_reflection(&self.memory_config)
    }

    async fn run_reflection(
        &mut self,
        trigger: ReflectionTaskTrigger,
        _messages: Vec<share::message::Message>,
        _run_id: &sdk::RunId,
        _run_step_id: Option<&sdk::RunStepId>,
        _cancel: CancellationToken,
    ) -> Result<ReflectionRunOutcome, LoopEngineError> {
        // 进入执行时 begin activity 必须已经发布且处于 Running。
        self.state_during_run = self
            .activities
            .snapshot()
            .activities
            .iter()
            .find(|activity| {
                matches!(activity.detail, sdk::ActivityDetailView::Reflection { .. })
            })
            .map(|activity| activity.state);
        self.runs.push(trigger);
        Ok(self.outcome.clone())
    }
}

struct IntervalRunResult {
    run: Run,
    activities: std::sync::Arc<crate::application::activity::ActivityCoordinator>,
    events: Vec<RuntimeLifecycleEvent>,
    reflection_runs: Vec<ReflectionTaskTrigger>,
    state_during_run: Option<sdk::ActivityStateView>,
}

/// 跑一次 text-only Complete 的完整 Main Run：`step_count` 由调用方指定以覆盖
/// interval 命中/未命中；反思端口按 `judgment` 判定、按 `outcome` 收口。
async fn drive_interval_run(
    step_count: usize,
    judgment: IntervalJudgment,
    outcome: ReflectionRunOutcome,
) -> IntervalRunResult {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(Vec::new(), step_count);
    let activities = std::sync::Arc::new(
        crate::application::activity::ActivityCoordinator::production_without_publisher(
            run.id().clone(),
            crate::application::activity::RunPurpose::Main,
        ),
    );
    let mut reflection = ReflectionFake {
        judgment,
        outcome,
        activities: activities.clone(),
        runs: Vec::new(),
        state_during_run: None,
        pre_compact_material: Default::default(),
        memory_config: share::config::MemoryConfig::default(),
    };
    let mut scenario = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        ..Default::default()
    };
    {
        let mut port = scenario.ports().run_loop();
        port.bind_activity_context(activities.clone(), "test-model".to_string());
        port.bind_reflection(&mut reflection);
        run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .expect("engine 必须正常完成：反思 outcome 不得终止宿主 Run");
    }
    let events = scenario.events();
    IntervalRunResult {
        run,
        activities,
        events,
        reflection_runs: reflection.runs,
        state_during_run: reflection.state_during_run,
    }
}

fn interval_outcome(status: ReflectionTaskCompletionStatus) -> ReflectionRunOutcome {
    ReflectionRunOutcome::Completed(ReflectionTaskCompletion {
        trigger: ReflectionTaskTrigger::Interval { step_count: 2 },
        status,
        metadata: None,
    })
}

fn transition_events(
    events: &[RuntimeLifecycleEvent],
) -> Vec<(RunStatus, RunStatus, RunTransitionReason)> {
    events
        .iter()
        .filter_map(|event| match event {
            RuntimeLifecycleEvent::Transitioned {
                from, to, reason, ..
            } => Some((*from, *to, *reason)),
            _ => None,
        })
        .collect()
}

fn transition_index(
    transitions: &[(RunStatus, RunStatus, RunTransitionReason)],
    from: RunStatus,
    to: RunStatus,
    reason: RunTransitionReason,
) -> Option<usize> {
    transitions
        .iter()
        .position(|(event_from, event_to, event_reason)| {
            *event_from == from && *event_to == to && *event_reason == reason
        })
}

/// 断言反思 phase 的完整转移闭环：进入前状态 → Reflecting → 进入前状态，
/// 且在 step 的 `ContinueAfterResponse` 收口之前。
fn assert_reflection_transition_cycle(result: &IntervalRunResult) {
    let transitions = transition_events(&result.events);
    let begin = transition_index(
        &transitions,
        RunStatus::ApplyingResponse,
        RunStatus::Reflecting,
        RunTransitionReason::BeginReflection,
    )
    .expect("反思必须发布 ApplyingResponse→Reflecting (BeginReflection) 转移");
    let completed = transition_index(
        &transitions,
        RunStatus::Reflecting,
        RunStatus::ApplyingResponse,
        RunTransitionReason::ReflectionCompleted,
    )
    .expect("反思必须发布 Reflecting→ApplyingResponse (ReflectionCompleted) 转移");
    assert!(
        begin < completed,
        "BeginReflection 必须先于 ReflectionCompleted；实际转移: {transitions:?}"
    );
    let close = transition_index(
        &transitions,
        RunStatus::ApplyingResponse,
        RunStatus::DrainingInput,
        RunTransitionReason::ContinueAfterResponse,
    )
    .expect("step 必须以 ContinueAfterResponse 收口");
    assert!(
        completed < close,
        "反思收口必须发生在 step 收口之前；实际转移: {transitions:?}"
    );
}

fn reflection_activities(
    activities: &crate::application::activity::ActivityCoordinator,
) -> Vec<sdk::ActivityView> {
    activities
        .snapshot()
        .activities
        .iter()
        .filter(|activity| matches!(activity.detail, sdk::ActivityDetailView::Reflection { .. }))
        .cloned()
        .collect()
}

#[tokio::test]
async fn interval_reflection_publishes_begin_and_terminal_activity_under_current_run() {
    let result = drive_interval_run(
        2,
        IntervalJudgment::Interval(2),
        interval_outcome(ReflectionTaskCompletionStatus::Succeeded),
    )
    .await;

    // ① Run 事件序列：ApplyingResponse→Reflecting→ApplyingResponse，reason 分别为
    //    BeginReflection/ReflectionCompleted，且都在 ContinueAfterResponse 收口之前。
    assert_reflection_transition_cycle(&result);

    // ② activity 快照：恰好一次 Reflection activity，parent = 当前 Run root，
    //    detail trigger = Interval，执行期 Running、终态 Succeeded。
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    // Run 收口后 root 不再 live，按 source=Run 取 root id（Reflection activity 在
    // 执行期挂到 activity_parent_id() == root 下）。
    let root_id = result
        .activities
        .snapshot()
        .activities
        .iter()
        .find(|activity| matches!(activity.source, sdk::ActivitySourceView::Run))
        .map(|activity| activity.id.clone())
        .expect("Run root activity 必须存在");
    assert_eq!(
        reflections[0].parent_activity_id.as_ref(),
        Some(&root_id),
        "Reflection activity 必须挂在当前 Run root 下"
    );
    assert_eq!(
        reflections[0].detail,
        sdk::ActivityDetailView::Reflection {
            trigger: sdk::ReflectionTriggerView::Interval,
        }
    );
    assert_eq!(
        result.state_during_run,
        Some(sdk::ActivityStateView::Running),
        "反思执行期间 begin activity 必须处于 Running"
    );
    assert_eq!(
        reflections[0].state,
        sdk::ActivityStateView::Succeeded,
        "Succeeded 反思必须以 Succeeded 终态收口 activity"
    );

    // ③ 端口收到 Interval 触发；step 随后正常 ContinueAfterResponse 收口，Run 正常推进。
    assert!(
        matches!(
            result.reflection_runs.as_slice(),
            [ReflectionTaskTrigger::Interval { step_count: 2 }]
        ),
        "端口必须收到 Interval{{ step_count: 2 }} 触发；实际: {:?}",
        result.reflection_runs
    );
    assert_eq!(result.run.status(), RunStatus::Completed);
    assert_eq!(result.run.steps().len(), 1);
    assert_eq!(
        result.run.steps()[0].invocation().unwrap().response(),
        "done"
    );
}

#[tokio::test]
async fn interval_reflection_failure_does_not_kill_run() {
    let result = drive_interval_run(
        2,
        IntervalJudgment::Interval(2),
        interval_outcome(ReflectionTaskCompletionStatus::Failed),
    )
    .await;

    // Failed outcome → activity 以 Failed 收口，但 Run 仍走完正常完成路径。
    assert_reflection_transition_cycle(&result);
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    assert_eq!(reflections[0].state, sdk::ActivityStateView::Failed);
    assert_eq!(result.run.status(), RunStatus::Completed);
    assert_eq!(result.run.steps().len(), 1);
}

#[tokio::test]
async fn interval_reflection_cancelled_records_cancelled_activity_and_continues() {
    let result = drive_interval_run(
        2,
        IntervalJudgment::Interval(2),
        interval_outcome(ReflectionTaskCompletionStatus::Cancelled),
    )
    .await;

    // 反思取消不杀宿主 Run（宿主 Run 的取消由既有 handle_interrupt 路径负责）：
    // activity 记录 Cancelled，Run 继续完成。
    assert_reflection_transition_cycle(&result);
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    assert_eq!(reflections[0].state, sdk::ActivityStateView::Cancelled);
    assert_eq!(result.run.status(), RunStatus::Completed);
    assert_eq!(result.run.steps().len(), 1);
}

#[tokio::test]
async fn interval_reflection_disabled_is_noop() {
    // 配置关闭时端口判定即返回 None（生产实现等价 DisabledSkipped 场景），engine 不进入 phase。
    let result = drive_interval_run(
        2,
        IntervalJudgment::Disabled,
        ReflectionRunOutcome::DisabledSkipped,
    )
    .await;

    let transitions = transition_events(&result.events);
    assert!(
        transition_index(
            &transitions,
            RunStatus::ApplyingResponse,
            RunStatus::Reflecting,
            RunTransitionReason::BeginReflection,
        )
        .is_none(),
        "禁用反思时不得进入 Reflecting；实际转移: {transitions:?}"
    );
    assert!(
        reflection_activities(&result.activities).is_empty(),
        "禁用反思时不得发布 Reflection activity"
    );
    assert!(
        result.reflection_runs.is_empty(),
        "禁用反思时端口执行不得被调用"
    );
    assert_eq!(result.run.status(), RunStatus::Completed);
}

#[tokio::test]
async fn interval_reflection_not_triggered_when_step_count_missed() {
    // step_count=3 未命中 interval=2：判定返回 None，无反思。
    let result = drive_interval_run(
        3,
        IntervalJudgment::Interval(2),
        interval_outcome(ReflectionTaskCompletionStatus::Succeeded),
    )
    .await;

    let transitions = transition_events(&result.events);
    assert!(
        transition_index(
            &transitions,
            RunStatus::ApplyingResponse,
            RunStatus::Reflecting,
            RunTransitionReason::BeginReflection,
        )
        .is_none(),
        "频控未命中时不得进入 Reflecting；实际转移: {transitions:?}"
    );
    assert!(
        reflection_activities(&result.activities).is_empty(),
        "频控未命中时不得发布 Reflection activity"
    );
    assert!(
        result.reflection_runs.is_empty(),
        "频控未命中时端口执行不得被调用"
    );
    assert_eq!(result.run.status(), RunStatus::Completed);
}

// ---------------------------------------------------------------------------
// PreCompact：自动压缩 Ready 后在 Compacting 内往返 Reflecting
// ---------------------------------------------------------------------------

struct PreCompactRunResult {
    run: Run,
    activities: std::sync::Arc<crate::application::activity::ActivityCoordinator>,
    events: Vec<RuntimeLifecycleEvent>,
    reflection_runs: Vec<ReflectionTaskTrigger>,
    state_during_run: Option<sdk::ActivityStateView>,
    slot: crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot,
}

fn pre_compact_outcome(status: ReflectionTaskCompletionStatus) -> ReflectionRunOutcome {
    ReflectionRunOutcome::Completed(ReflectionTaskCompletion {
        trigger: ReflectionTaskTrigger::PreCompact,
        status,
        metadata: None,
    })
}

/// 跑一次「自动压缩触发 → PreCompact 反思」的完整 Main Run。
///
/// `material` 是压缩 Committed 时观察者将暂存进共享槽的被丢弃消息；`None` 模拟
/// `CompactOutcome::Skipped`（观察者不暂存）。生产链路为
/// `ChatCompactionObserver::on_compacted` 暂存 → engine 反思 phase 取出。
async fn drive_pre_compact_run(
    material: Option<Vec<share::message::Message>>,
    memory_config: share::config::MemoryConfig,
) -> PreCompactRunResult {
    let slot = crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot::default();
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(Vec::new(), 1);
    let activities = std::sync::Arc::new(
        crate::application::activity::ActivityCoordinator::production_without_publisher(
            run.id().clone(),
            crate::application::activity::RunPurpose::Main,
        ),
    );
    let mut reflection = ReflectionFake {
        // PreCompact 场景关闭 Interval 判定，聚焦压缩材料路径。
        judgment: IntervalJudgment::Disabled,
        outcome: pre_compact_outcome(ReflectionTaskCompletionStatus::Succeeded),
        activities: activities.clone(),
        runs: Vec::new(),
        state_during_run: None,
        pre_compact_material: slot.clone(),
        memory_config,
    };
    let mut scenario = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        needs_compaction: true,
        pre_compact_material: material,
        pre_compact_slot: Some(slot.clone()),
        ..Default::default()
    };
    {
        let mut port = scenario.ports().run_loop();
        port.bind_activity_context(activities.clone(), "test-model".to_string());
        port.bind_reflection(&mut reflection);
        run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .expect("engine 必须正常完成：PreCompact 反思不得终止宿主 Run");
    }
    PreCompactRunResult {
        run,
        activities,
        events: scenario.events(),
        reflection_runs: reflection.runs,
        state_during_run: reflection.state_during_run,
        slot,
    }
}

/// 无反思断言：不进入 Reflecting、不发布 Reflection activity、端口执行不被调用。
fn assert_no_reflection_phase(
    events: &[RuntimeLifecycleEvent],
    activities: &crate::application::activity::ActivityCoordinator,
    reflection_runs: &[ReflectionTaskTrigger],
    context: &str,
) {
    let transitions = transition_events(events);
    assert!(
        transition_index(
            &transitions,
            RunStatus::Compacting,
            RunStatus::Reflecting,
            RunTransitionReason::BeginReflection,
        )
        .is_none(),
        "{context}：不得进入 Compacting→Reflecting；实际转移: {transitions:?}"
    );
    assert!(
        reflection_activities(activities).is_empty(),
        "{context}：不得发布 Reflection activity"
    );
    assert!(
        reflection_runs.is_empty(),
        "{context}：反思端口执行不得被调用"
    );
}

#[tokio::test]
async fn pre_compact_reflection_runs_inside_compacting_state_with_activity() {
    // 压缩返回 Committed：观察者暂存被丢弃消息 → engine 在 Compacting 内取出执行。
    let result = drive_pre_compact_run(
        Some(vec![
            share::message::Message::user("discarded-1"),
            share::message::Message::user("discarded-2"),
        ]),
        share::config::MemoryConfig::default(),
    )
    .await;

    // ① 事件序列：BeginCompaction 进入 Compacting 后，Reflecting 往返完整嵌在
    //    Compacting 内，且在 CompactionCompleted 之前。
    let transitions = transition_events(&result.events);
    let begin_compact = transition_index(
        &transitions,
        RunStatus::PreparingContext,
        RunStatus::Compacting,
        RunTransitionReason::BeginCompaction,
    )
    .expect("必须发布 PreparingContext→Compacting (BeginCompaction) 转移");
    let begin_reflect = transition_index(
        &transitions,
        RunStatus::Compacting,
        RunStatus::Reflecting,
        RunTransitionReason::BeginReflection,
    )
    .expect("压缩材料暂存后必须进入 Compacting→Reflecting (BeginReflection)");
    let complete_reflect = transition_index(
        &transitions,
        RunStatus::Reflecting,
        RunStatus::Compacting,
        RunTransitionReason::ReflectionCompleted,
    )
    .expect("反思必须返回 Compacting (ReflectionCompleted)");
    let complete_compact = transition_index(
        &transitions,
        RunStatus::Compacting,
        RunStatus::PreparingContext,
        RunTransitionReason::CompactionCompleted,
    )
    .expect("反思收口后必须发布 Compacting→PreparingContext (CompactionCompleted)");
    assert!(
        begin_compact < begin_reflect
            && begin_reflect < complete_reflect
            && complete_reflect < complete_compact,
        "Reflecting 往返必须嵌在 Compacting 内且先于 CompactionCompleted；实际转移: {transitions:?}"
    );

    // ② activity：恰好一次 Reflection{trigger:PreCompact}，parent = 当前 Run root，
    //    执行期 Running、终态 Succeeded。
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    let root_id = result
        .activities
        .snapshot()
        .activities
        .iter()
        .find(|activity| matches!(activity.source, sdk::ActivitySourceView::Run))
        .map(|activity| activity.id.clone())
        .expect("Run root activity 必须存在");
    assert_eq!(
        reflections[0].parent_activity_id.as_ref(),
        Some(&root_id),
        "Reflection activity 必须挂在当前 Run root 下"
    );
    assert_eq!(
        reflections[0].detail,
        sdk::ActivityDetailView::Reflection {
            trigger: sdk::ReflectionTriggerView::PreCompact,
        }
    );
    assert_eq!(
        result.state_during_run,
        Some(sdk::ActivityStateView::Running),
        "反思执行期间 begin activity 必须处于 Running"
    );
    assert_eq!(
        reflections[0].state,
        sdk::ActivityStateView::Succeeded,
        "Succeeded 反思必须以 Succeeded 终态收口 activity"
    );

    // ③ 端口收到 PreCompact 触发；Run 正常推进。
    assert!(
        matches!(
            result.reflection_runs.as_slice(),
            [ReflectionTaskTrigger::PreCompact]
        ),
        "端口必须收到 PreCompact 触发；实际: {:?}",
        result.reflection_runs
    );
    assert_eq!(result.run.status(), RunStatus::Completed);
    assert_eq!(result.run.steps().len(), 1);
    assert_eq!(
        result.run.steps()[0].invocation().unwrap().response(),
        "done"
    );
    assert!(
        result.slot.staged().is_none(),
        "材料被取出执行后槽位必须清空"
    );
}

#[tokio::test]
async fn pre_compact_reflection_skipped_when_compact_skipped() {
    // 压缩返回 Skipped：观察者不暂存材料 → engine 取不到材料，不进反思 phase。
    let result = drive_pre_compact_run(None, share::config::MemoryConfig::default()).await;

    assert_no_reflection_phase(
        &result.events,
        &result.activities,
        &result.reflection_runs,
        "compact Skipped",
    );
    // 压缩本身照常收口（Skipped 在生产中也是 Ok(Ready) 路径），只是不带反思往返。
    let transitions = transition_events(&result.events);
    assert!(
        transition_index(
            &transitions,
            RunStatus::Compacting,
            RunStatus::PreparingContext,
            RunTransitionReason::CompactionCompleted,
        )
        .is_some(),
        "Skipped 也必须照常收口 CompactionCompleted；实际转移: {transitions:?}"
    );
    assert_eq!(result.run.status(), RunStatus::Completed);
    assert_eq!(result.run.steps().len(), 1);
    assert!(result.slot.staged().is_none());
}

#[tokio::test]
async fn pre_compact_reflection_disabled_drops_material_without_phase() {
    // Committed 已暂存材料，但反思配置关闭：取出即丢弃，不进 phase、不滞留。
    let disabled = share::config::MemoryConfig {
        enabled: false,
        ..Default::default()
    };
    let result = drive_pre_compact_run(
        Some(vec![share::message::Message::user("discarded")]),
        disabled,
    )
    .await;

    assert_no_reflection_phase(
        &result.events,
        &result.activities,
        &result.reflection_runs,
        "反思配置关闭",
    );
    assert!(
        result.slot.staged().is_none(),
        "反思禁用时暂存材料必须被丢弃，NEVER 滞留到下一次 compact"
    );
    assert_eq!(result.run.status(), RunStatus::Completed);
    assert_eq!(result.run.steps().len(), 1);
}

// ---------------------------------------------------------------------------
// Manual：/reflect-now 走真 Run——Created→DrainingInput→Reflecting→DrainingInput→Completed
// ---------------------------------------------------------------------------

use crate::application::loop_engine::{ManualReflectionOutcome, ManualReflectionPort};

/// 手动反思端口的脚本化终态；`PortError` 模拟端口 Err（契约违约）。
enum ManualReflectionScript {
    Ready(ReflectionTaskCompletionStatus),
    Cancelled,
    TimedOut,
    PortError,
}

/// 脚本化手动反思端口：记录执行次数与执行期 activity 状态。
struct ManualReflectionFake {
    script: ManualReflectionScript,
    activities: std::sync::Arc<crate::application::activity::ActivityCoordinator>,
    calls: usize,
    state_during_run: Option<sdk::ActivityStateView>,
}

#[async_trait::async_trait]
impl ManualReflectionPort for ManualReflectionFake {
    async fn run_manual_reflection(
        &mut self,
        _run_id: &sdk::RunId,
        _cancel: &CancellationToken,
    ) -> Result<ManualReflectionOutcome, LoopEngineError> {
        // 执行期 Reflection activity 必须已发布且处于 Running。
        self.state_during_run = reflection_activities(&self.activities)
            .first()
            .map(|activity| activity.state);
        self.calls += 1;
        match &self.script {
            ManualReflectionScript::Ready(status) => {
                Ok(ManualReflectionOutcome::Ready(*status))
            }
            ManualReflectionScript::Cancelled => Ok(ManualReflectionOutcome::Cancelled),
            ManualReflectionScript::TimedOut => Ok(ManualReflectionOutcome::TimedOut),
            ManualReflectionScript::PortError => {
                Err(LoopEngineError::Adapter("手动反思端口故障".to_string()))
            }
        }
    }
}

struct ManualReflectionRunResult {
    run: Run,
    activities: std::sync::Arc<crate::application::activity::ActivityCoordinator>,
    events: Vec<RuntimeLifecycleEvent>,
    calls: usize,
    state_during_run: Option<sdk::ActivityStateView>,
    directive: Result<LoopDirective, LoopEngineError>,
    observed_calls: Vec<&'static str>,
}

/// 跑一次完整的 Manual Reflection Run：无 accepted 输入（空 drain 脚本），
/// 反思由 engine 的 `execute_manual_reflection` 在主循环之前执行。
async fn drive_manual_reflection_run(
    script: ManualReflectionScript,
) -> ManualReflectionRunResult {
    let mut run = Run::new(RunSpec::manual_reflection(), None);
    let cancel = CancellationToken::new();
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(Vec::new(), 0);
    let activities = std::sync::Arc::new(
        crate::application::activity::ActivityCoordinator::production_without_publisher(
            run.id().clone(),
            crate::application::activity::RunPurpose::Reflection,
        ),
    );
    let mut fake = ManualReflectionFake {
        script,
        activities: activities.clone(),
        calls: 0,
        state_during_run: None,
    };
    let mut scenario = ScriptedScenario {
        drain_outcomes: manual_compaction_drain_outcomes(),
        ..Default::default()
    };
    let directive = {
        let mut port = scenario.ports().run_loop();
        port.bind_activity_context(activities.clone(), "test-model".to_string());
        port.bind_manual_reflection(&mut fake);
        run_loop(&mut run, &mut execution, &cancel, &mut port).await
    };
    ManualReflectionRunResult {
        run,
        activities,
        events: scenario.events(),
        calls: fake.calls,
        state_during_run: fake.state_during_run,
        directive,
        observed_calls: scenario.calls(),
    }
}

fn assert_manual_reflection_round_trip(result: &ManualReflectionRunResult) {
    let transitions = transition_events(&result.events);
    let draining = transition_index(
        &transitions,
        RunStatus::Created,
        RunStatus::DrainingInput,
        RunTransitionReason::DrainStarted,
    )
    .expect("手动反思 Run 必须发布 Created→DrainingInput (DrainStarted)");
    let begin = transition_index(
        &transitions,
        RunStatus::DrainingInput,
        RunStatus::Reflecting,
        RunTransitionReason::BeginReflection,
    )
    .expect("手动反思必须发布 DrainingInput→Reflecting (BeginReflection)");
    let settle = transition_index(
        &transitions,
        RunStatus::Reflecting,
        RunStatus::DrainingInput,
        RunTransitionReason::ManualReflectionSettled,
    )
    .expect("反思收口必须回到 DrainingInput (ManualReflectionSettled)");
    let completed = transition_index(
        &transitions,
        RunStatus::DrainingInput,
        RunStatus::Completed,
        RunTransitionReason::DrainEmptyAndSealed,
    )
    .expect("Completed 的唯一来源必须是 drain 的 EmptyAndSealed");
    assert!(
        draining < begin && begin < settle && settle < completed,
        "lifecycle 必须是 Created→DrainingInput→Reflecting→DrainingInput→Completed；实际转移: {transitions:?}"
    );
}

#[tokio::test]
async fn manual_reflection_run_completes_reflecting_round_trip_without_model() {
    let result = drive_manual_reflection_run(ManualReflectionScript::Ready(
        ReflectionTaskCompletionStatus::Succeeded,
    ))
    .await;

    match &result.directive {
        Ok(LoopDirective::Terminal) => {}
        other => panic!("手动反思 Run 应以 Terminal 收口: {other:?}"),
    }
    assert_eq!(result.run.status(), RunStatus::Completed);

    // ① lifecycle 完整闭环。
    assert_manual_reflection_round_trip(&result);

    // ② root purpose=Reflection；Reflection{trigger:Manual} 执行期 Running、终态 Succeeded。
    let snapshot = result.activities.snapshot();
    let root = snapshot
        .activities
        .iter()
        .find(|activity| activity.kind == sdk::ActivityKindView::Run)
        .expect("Run root activity 必须存在");
    assert_eq!(
        root.detail,
        sdk::ActivityDetailView::Run {
            purpose: sdk::RunPurposeView::Reflection,
        },
        "手动反思 Run 的根 activity 必须携带 Reflection 目的"
    );
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    assert_eq!(
        reflections[0].parent_activity_id.as_ref(),
        Some(&root.id),
        "Reflection activity 必须挂在当前 Run root 下"
    );
    assert_eq!(
        reflections[0].detail,
        sdk::ActivityDetailView::Reflection {
            trigger: sdk::ReflectionTriggerView::Manual,
        }
    );
    assert_eq!(
        result.state_during_run,
        Some(sdk::ActivityStateView::Running),
        "反思执行期间 activity 必须处于 Running"
    );
    assert_eq!(reflections[0].state, sdk::ActivityStateView::Succeeded);

    // ③ 无 RunStep、无模型调用；端口恰好执行一次。
    assert_eq!(result.calls, 1, "手动反思端口必须恰好执行一次");
    assert!(
        result.run.steps().is_empty(),
        "手动反思 Run 不得创建 RunStep"
    );
    assert!(
        !result.observed_calls.contains(&"model"),
        "手动反思 Run 不得调用模型: {:?}",
        result.observed_calls
    );
}

#[tokio::test]
async fn manual_reflection_port_error_settles_activity_and_state_before_propagating() {
    let result = drive_manual_reflection_run(ManualReflectionScript::PortError).await;

    match &result.directive {
        Err(LoopEngineError::Adapter(message)) => {
            assert!(message.contains("手动反思"), "错误信息应指明手动反思端口: {message}");
        }
        other => panic!("端口 Err 必须上抛: {other:?}"),
    }
    assert_eq!(result.calls, 1);

    // 状态机必须收口回 DrainingInput，NEVER 悬挂 Reflecting。
    let transitions = transition_events(&result.events);
    transition_index(
        &transitions,
        RunStatus::Reflecting,
        RunStatus::DrainingInput,
        RunTransitionReason::ManualReflectionSettled,
    )
    .expect("端口 Err 也必须先把 Reflecting 收口回 DrainingInput");
    assert_eq!(
        result.run.status(),
        RunStatus::DrainingInput,
        "端口 Err 上抛前状态机必须已收口"
    );

    // activity 必须按 Failed 收口，NEVER 留下 Running 悬挂。
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    assert_eq!(reflections[0].state, sdk::ActivityStateView::Failed);
    assert!(
        !result.observed_calls.contains(&"model"),
        "端口 Err 路径不得调用模型: {:?}",
        result.observed_calls
    );
}

#[tokio::test]
async fn manual_reflection_cancelled_terminates_run_and_closes_activity() {
    let result = drive_manual_reflection_run(ManualReflectionScript::Cancelled).await;

    match &result.directive {
        Ok(LoopDirective::Terminal) => {}
        other => panic!("取消的手动反思 Run 应以 Terminal 收口: {other:?}"),
    }
    assert_eq!(result.run.status(), RunStatus::Terminated);
    assert_manual_reflection_round_trip_status(&result, RunStatus::Terminated);
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    assert_eq!(reflections[0].state, sdk::ActivityStateView::Cancelled);
    assert_eq!(result.calls, 1);
}

#[tokio::test]
async fn manual_reflection_timeout_fails_run_and_closes_activity() {
    let result = drive_manual_reflection_run(ManualReflectionScript::TimedOut).await;

    match &result.directive {
        Ok(LoopDirective::Terminal) => {}
        other => panic!("超时的手动反思 Run 应以 Terminal 收口: {other:?}"),
    }
    assert_eq!(result.run.status(), RunStatus::Failed);
    let reflections = reflection_activities(&result.activities);
    assert_eq!(reflections.len(), 1, "必须恰好发布一次 Reflection activity");
    assert_eq!(reflections[0].state, sdk::ActivityStateView::Terminated);
    assert_eq!(result.calls, 1);
}

/// 取消/超时路径也必须先完成 DrainingInput→Reflecting 的入口转移。
fn assert_manual_reflection_round_trip_status(
    result: &ManualReflectionRunResult,
    final_status: RunStatus,
) {
    let transitions = transition_events(&result.events);
    transition_index(
        &transitions,
        RunStatus::DrainingInput,
        RunStatus::Reflecting,
        RunTransitionReason::BeginReflection,
    )
    .expect("手动反思必须发布 DrainingInput→Reflecting (BeginReflection)");
    assert_eq!(result.run.status(), final_status);
}
