/// Interval 反思执行点上移 engine 后的 L2 协作测试。
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
            } => Some((from.clone(), to.clone(), reason.clone())),
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
