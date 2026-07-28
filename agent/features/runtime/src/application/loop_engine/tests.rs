use super::*;
use crate::application::interaction::port::{InteractionBridge, InteractionPort};
use crate::application::tool::agent::ToolCall;
use sdk::{ChatInputEvent, InteractionRequest};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[test]
fn execution_state_is_owned_by_engine_and_not_exposed_as_a_port() {
    let engine_source = include_str!("engine.rs");
    let main_source = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub_source = include_str!("../run/derived/loop_run.rs");

    assert!(!engine_source.contains("pub trait ExecutionStatePort"));
    assert!(!engine_source.contains("execution_state_mut"));
    assert!(!engine_source.contains("std::mem::swap"));
    assert!(!engine_source.contains("pub trait RunLoopPort"));
    assert!(!main_source
        .contains("pub execution: crate::application::run::execution_state::RunExecutionState"));
    assert!(!sub_source.contains("pub(super) struct SubRunCapabilities<'a> {\n    pub execution:"));
}

#[test]
fn production_engine_owns_step_state_reset() {
    let source = include_str!("engine.rs");
    assert!(source.contains("execution.begin_step()"));
    assert!(source.contains("run_loop(run, execution, cancel, port)"));

    let main_source = include_str!("../loop_engine/chat/loop_runner.rs");
    let sub_source = include_str!("../run/derived/loop_run.rs");
    assert!(main_source.contains("crate::application::run::launcher::launch_prepared("));
    assert!(sub_source.contains("crate::application::run::launcher::launch_prepared("));
    assert!(!main_source.contains("loop_engine::run_loop("));
    assert!(!sub_source.contains("loop_engine::run_loop("));
}

#[test]
fn production_engine_entry_uses_supplied_execution_and_context() {
    let source = include_str!("engine.rs");
    assert!(!source.contains("let _ = (execution, context)"));
    assert!(source.contains("run_loop(run, execution, cancel, port)"));
    assert!(source.contains("context.event_sink()") || source.contains("context.input()"));
}

use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, InternalContinuationKind, LoopInput,
};

#[test]
fn p6_9_engine_has_no_fat_capability_aggregation_trait() {
    let engine = include_str!("engine.rs");
    let launcher = include_str!("../run/launcher.rs");
    let main = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub = include_str!("../run/derived/loop_run.rs");
    let sub_setup = include_str!("../run/derived/setup.rs");

    assert!(
        !engine.contains("pub trait LoopEnginePort"),
        "narrow stage capabilities must not be re-aggregated into LoopEnginePort"
    );
    assert!(!launcher.contains("LoopEnginePort"));
    for retired in [
        "MainRunPort",
        "SubAgentRun",
        "SubAgentLaunch",
        "DerivedSubRun",
    ] {
        assert!(
            !main.contains(retired) && !sub.contains(retired) && !sub_setup.contains(retired),
            "retired role-shaped execution shell remains: {retired}"
        );
    }
    assert!(main.contains("struct MainRunCapabilities"));
    assert!(sub.contains("struct SubRunCapabilities"));
    assert!(sub.contains("async fn launch_sub_run("));
}

#[test]
fn stop_hook_is_published_as_narrow_port() {
    let source = include_str!("engine.rs");
    assert!(source.contains("pub trait StopHookPort: Send"));
    assert!(source.contains("fn stop_hook_context(&self) -> StopHookContext"));
    assert!(source.contains("async fn project_stop_hook_outcome"));
    assert!(source.contains("orchestrate_stop_hook("));
}

#[test]
fn p6_3_model_invocation_has_one_shared_orchestration() {
    let coordinator = include_str!("../model/invocation.rs");
    let main = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub = include_str!("../run/derived/loop_run.rs");

    assert!(
        coordinator.contains("pub(crate) async fn orchestrate_model_invocation"),
        "model coordinator must own the complete invocation orchestration"
    );
    assert_eq!(
        coordinator.matches("async fn invoke_model_impl(").count(),
        1,
        "the shared coordinator must contain the only invoke_model_impl"
    );
    for (name, source) in [("Main", main), ("Sub", sub)] {
        assert!(
            !source.contains("async fn invoke_model_impl"),
            "{name} must not retain a role-specific model invocation algorithm"
        );
        assert!(
            !source.contains("ModelInvocationCoordinator::new()"),
            "{name} must not drive retry/stream orchestration directly"
        );
        assert!(
            !source.contains("provider.invoke("),
            "{name} must not invoke the provider directly"
        );
    }
}

#[test]
fn p6_8_agent_dispatch_has_no_direct_completion_bypass() {
    let dispatch = include_str!("../../../../tools/src/domain/agent_port.rs");
    let sub_runner = include_str!("../run/derived/setup.rs");
    let sub_type = include_str!("../run/derived.rs");

    assert!(dispatch.contains("async fn run_agent("));
    assert!(
        !dispatch.contains("async fn complete("),
        "AgentDispatch must not expose an agentless completion path"
    );
    assert!(
        !sub_runner.contains("binding.provider.invoke("),
        "Sub runner must not invoke a provider outside the shared Run Engine"
    );
    assert!(
        !sub_type.contains("pub config_reader:"),
        "CliAgentRunner must not retain config state solely for direct completion"
    );
}

#[test]
fn p6_6_step_transaction_calculation_has_single_engine_owner() {
    let engine = include_str!("engine.rs");
    let main_adapter = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub_adapter = include_str!("../run/derived/loop_run.rs");

    assert!(engine.contains("struct StepCommit"));
    assert!(engine.contains("fn prepare_step_commit"));
    for adapter in [main_adapter, sub_adapter] {
        assert!(!adapter.contains("committed_message_count() + execution.accepted_input_len()"));
        assert!(!adapter.contains("execution.commit_all_messages()"));
        assert!(!adapter.contains("execution.commit_step_messages()"));
    }
}

use crate::domain::agent_run::{
    InteractionContinuation, Run, RunControl, RunDomainEvent, RunSpec, RunStatus, ToolCallStatus,
};

/// #1248: Fake ToolExecutionPort that counts execute calls and returns configurable results.
/// Used for production-level tests of the approval flow through the full engine roundtrip.
#[derive(Clone)]
struct FakeToolExecutionPort {
    execute_count: Arc<std::sync::atomic::AtomicUsize>,
    recorded_invocations: Arc<std::sync::Mutex<Vec<tools::ToolInvocation>>>,
    result_text: Arc<std::sync::Mutex<String>>,
    /// Records the last text returned by execute() so tests can assert on it.
    returned_text: Arc<std::sync::Mutex<Option<String>>>,
}

impl FakeToolExecutionPort {
    fn new() -> Self {
        Self {
            execute_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            recorded_invocations: Arc::new(std::sync::Mutex::new(Vec::new())),
            result_text: Arc::new(std::sync::Mutex::new(String::new())),
            returned_text: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn execute_count(&self) -> usize {
        self.execute_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn set_result_text(&self, text: &str) {
        *self.result_text.lock().unwrap() = text.to_string();
    }

    fn returned_text(&self) -> Option<String> {
        self.returned_text.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl tools::ToolExecutionPort for FakeToolExecutionPort {
    async fn execute(
        &self,
        invocation: tools::ToolInvocation,
        cancellation: &dyn tools::CancellationSignal,
    ) -> tools::ToolExecutionOutcome {
        let _ = cancellation;
        self.execute_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.recorded_invocations.lock().unwrap().push(invocation);
        let text = self.result_text.lock().unwrap().clone();
        let outcome_text = if text.is_empty() {
            "fake result".to_string()
        } else {
            text.clone()
        };
        *self.returned_text.lock().unwrap() = Some(outcome_text.clone());
        tools::ToolExecutionOutcome::success_text(outcome_text)
    }
}

struct ScriptedPort {
    model_steps: VecDeque<ModelStep>,
    model_errors: VecDeque<LoopEngineError>,
    tool_steps: VecDeque<ToolStep>,
    controls: std::sync::Mutex<VecDeque<RunControl>>,
    registered_step: std::sync::Mutex<Option<sdk::RunStepId>>,
    cancel_when_compact_starts: bool,
    cancel_when_model_starts: bool,
    cancel_when_tools_starts: bool,
    terminate_when_compact_starts: bool,
    step_cancel: std::sync::Arc<std::sync::Mutex<Option<CancellationToken>>>,
    calls: Vec<&'static str>,
    events: Vec<RunDomainEvent>,
    guarded_calls: Vec<Vec<ToolGuardDecision>>,
    drain_outcomes: VecDeque<DrainOutcome>,
    /// #1272: ScriptedPort tracks its own drain epoch for validation.
    /// On each `drain_input` call, validates the engine's expected_epoch
    /// against this counter and advances it after a successful drain.
    drain_epoch: DrainEpoch,
    cancelled_during_model: bool,
    block_model_forever: bool,
    block_compact_until_cancelled: bool,
    cancelled_steps: Vec<sdk::RunStepId>,
    finalized_steps: Vec<sdk::RunStepId>,
    frozen_steps: Vec<sdk::RunStepId>,
    fail_accept_input: bool,
    needs_compaction: bool,
    fail_emit_once: bool,
    /// #1248 Task 5: Test interaction bridge used by `interaction_port()`.
    interaction_bridge: Arc<InteractionBridge>,
    /// #1248 Task 5: Records published interaction requests.
    published_interactions: Arc<std::sync::Mutex<Vec<InteractionRequest>>>,
    /// #1248: Pending interaction work stored by the engine.
    pending_work: std::sync::Mutex<Option<super::engine::PendingInteractionWork>>,
    /// #1248: Fake tool execution port for counting/tracking approvals.
    fake_tool_port: Option<Arc<FakeToolExecutionPort>>,
}

impl Default for ScriptedPort {
    fn default() -> Self {
        let mut drain_outcomes = VecDeque::new();
        drain_outcomes.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "test-input".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_outcomes.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });
        Self {
            model_steps: Default::default(),
            model_errors: Default::default(),
            tool_steps: Default::default(),
            controls: Default::default(),
            registered_step: Default::default(),
            cancel_when_compact_starts: false,
            cancel_when_model_starts: false,
            cancel_when_tools_starts: false,
            terminate_when_compact_starts: false,
            step_cancel: Default::default(),
            calls: Default::default(),
            events: Default::default(),
            guarded_calls: Default::default(),
            drain_outcomes,
            drain_epoch: DrainEpoch(0),
            cancelled_during_model: false,
            block_model_forever: false,
            block_compact_until_cancelled: false,
            cancelled_steps: Default::default(),
            finalized_steps: Default::default(),
            frozen_steps: Default::default(),
            fail_accept_input: false,
            needs_compaction: false,
            fail_emit_once: false,
            interaction_bridge: Arc::new(InteractionBridge::new()),
            published_interactions: Arc::new(std::sync::Mutex::new(Vec::new())),
            pending_work: std::sync::Mutex::new(None),
            fake_tool_port: None,
        }
    }
}

#[async_trait::async_trait]
impl InputPort for ScriptedPort {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.calls.push("input");
        if expected_epoch != self.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.drain_epoch,
            )));
        }
        let outcome =
            self.drain_outcomes
                .pop_front()
                .unwrap_or_else(|| DrainOutcome::EmptyAndSealed {
                    epoch: self.drain_epoch,
                });
        if !matches!(&outcome, DrainOutcome::NoInput { .. }) {
            self.drain_epoch = self.drain_epoch.next();
        }
        Ok(outcome)
    }

    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.calls.push("await_input");
        if expected_epoch != self.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.drain_epoch,
            )));
        }
        let outcome = self
            .drain_outcomes
            .pop_front()
            .unwrap_or_else(|| DrainOutcome::NoInput {
                epoch: self.drain_epoch,
            });
        if !matches!(
            &outcome,
            DrainOutcome::EmptyAndSealed { .. } | DrainOutcome::NoInput { .. }
        ) {
            self.drain_epoch = self.drain_epoch.next();
        }
        Ok(outcome)
    }
}

#[async_trait::async_trait]
impl EventSinkPort for ScriptedPort {
    async fn emit(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("emit");
        if self.fail_emit_once {
            self.fail_emit_once = false;
            return Err(LoopEngineError::Adapter("emit failed".to_string()));
        }
        self.events.extend(events);
        Ok(())
    }
}

#[async_trait::async_trait]
impl RunControlPort for ScriptedPort {
    fn take_control(&self, _run_id: &sdk::RunId) -> Option<RunControl> {
        self.controls.lock().unwrap().pop_front()
    }
}

#[async_trait::async_trait]
impl RunLifecyclePort for ScriptedPort {
    fn claim_terminal(&self, _run_id: &sdk::RunId) -> bool {
        true
    }

    fn claim_cancellation(&self, _run_id: &sdk::RunId) -> bool {
        true
    }

    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        let _ = run_id;
        *self.registered_step.lock().unwrap() = Some(step_id);
        *self.step_cancel.lock().unwrap() = Some(cancel);
    }
}

#[async_trait::async_trait]
impl StepPersistencePort for ScriptedPort {
    fn observe_step_frozen(&mut self, step_id: &sdk::RunStepId) {
        self.calls.push("freeze_step");
        self.frozen_steps.push(step_id.clone());
    }

    fn build_context_request(
        &self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        _run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Option<crate::ports::ContextRequest> {
        let _ = step_id;
        None
    }

    async fn accept_step_input(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("accept_step_input");
        if self.fail_accept_input {
            return Err(LoopEngineError::Adapter(
                "accepted input durable write failed".to_string(),
            ));
        }
        Ok(())
    }

    async fn persist_step_commit(
        &mut self,
        commit: &super::engine::StepCommit,
    ) -> Result<(), LoopEngineError> {
        match commit.cause {
            crate::ports::FinalizeCause::Completed => {
                self.calls.push("finalize_step");
                self.finalized_steps.push(commit.step_id.clone());
            }
            crate::ports::FinalizeCause::UserCancelledStep
            | crate::ports::FinalizeCause::RunTerminated => {
                self.calls.push("finalize_cancelled_step");
                self.cancelled_steps.push(commit.step_id.clone());
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CompactionPort for ScriptedPort {
    async fn needs_compaction(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        self.calls.push("needs_compaction");
        Ok(self.needs_compaction)
    }

    async fn compact(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("compact");
        if self.cancel_when_compact_starts {
            let step_id = self
                .registered_step
                .lock()
                .unwrap()
                .clone()
                .expect("step scope must be registered before compact");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
        }
        if self.terminate_when_compact_starts {
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::Terminate {
                    reason: sdk::RunTerminationReason::UserExit,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_456),
                });
            cancel.cancel();
        }
        if self.block_compact_until_cancelled {
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ModelInvocationPort for ScriptedPort {
    async fn invoke_model(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        self.calls.push("model");
        if self.cancel_when_model_starts {
            let step_id = self
                .registered_step
                .lock()
                .unwrap()
                .clone()
                .expect("step scope must be registered before model");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
            return Err(LoopEngineError::Cancelled);
        }
        if self.block_model_forever {
            std::future::pending::<()>().await;
        }
        if self.cancelled_during_model {
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        if let Some(error) = self.model_errors.pop_front() {
            return Err(error);
        }
        self.model_steps
            .pop_front()
            .map(|step| (step, StepTokenUsage::default()))
            .ok_or_else(|| LoopEngineError::Adapter("missing model step".to_string()))
    }
}

#[async_trait::async_trait]
impl StopHookPort for ScriptedPort {}

#[async_trait::async_trait]
impl ToolOrchestrationPort for ScriptedPort {
    async fn execute_tools(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        self.calls.push("tools");
        self.guarded_calls
            .push(calls.iter().map(|(_, decision)| decision.clone()).collect());
        if self.cancel_when_tools_starts {
            let step_id = self
                .registered_step
                .lock()
                .unwrap()
                .clone()
                .expect("step scope must be registered before tools");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
            return Err(LoopEngineError::Cancelled);
        }
        self.tool_steps
            .pop_front()
            .map(
                |step| crate::application::tool::coordination::ToolRoundOutcome {
                    continuation: if matches!(
                        step,
                        ToolStep::Continue | ToolStep::ContinueWithFuseBypass(_)
                    ) {
                        crate::application::tool::coordination::ToolRoundContinuation::ToolResults
                    } else {
                        crate::application::tool::coordination::ToolRoundContinuation::None
                    },
                    step,
                },
            )
            .ok_or_else(|| LoopEngineError::Adapter("missing tool step".to_string()))
    }
}

#[async_trait::async_trait]
impl StuckHandlingPort for ScriptedPort {
    async fn on_stuck(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        _decision: &StuckDecision,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("stuck");
        Ok(())
    }
}

impl PlanApprovalPort for ScriptedPort {}

#[async_trait::async_trait]
impl InteractionCompletionPort for ScriptedPort {
    fn interaction_execution_scope(&self) -> tools::ExecutionScope {
        tools::ExecutionScope::builder(
            "test-run".to_string(),
            project::WorkspaceId::from("test-ws".to_string()),
            std::path::PathBuf::from("/tmp"),
        )
        .build()
    }

    fn interaction_tool_execution(&self) -> &dyn tools::ToolExecutionPort {
        self.fake_tool_port
            .as_deref()
            .expect("interaction test requires fake tool execution")
    }

    fn interaction_materializer(
        &self,
    ) -> &crate::application::tool::result_materialization::ToolResultMaterializer {
        static MATERIALIZER: std::sync::OnceLock<
            std::sync::Arc<
                crate::application::tool::result_materialization::ToolResultMaterializer,
            >,
        > = std::sync::OnceLock::new();
        MATERIALIZER
            .get_or_init(crate::application::tool::test_support::test_tool_result_materializer)
            .as_ref()
    }

    fn interaction_session_id(&self) -> &str {
        "test-session"
    }

    fn interaction_cancellation(
        &self,
        step_cancel: CancellationToken,
    ) -> std::sync::Arc<dyn tools::CancellationSignal> {
        std::sync::Arc::new(
            crate::application::run::context::RunCancellationScope::from_token(step_cancel),
        )
    }
}

#[async_trait::async_trait]
impl InteractionMailboxPort for ScriptedPort {
    fn interaction_port(&self) -> &dyn InteractionPort {
        self.interaction_bridge.as_ref()
    }

    async fn publish_interaction(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        request: &InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("publish_interaction");
        self.published_interactions
            .lock()
            .unwrap()
            .push(request.clone());
        Ok(())
    }

    fn set_pending_interaction_work(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        work: super::engine::PendingInteractionWork,
    ) {
        self.calls.push("set_pending_interaction_work");
        execution.set_pending_interaction_work(work.clone());
        *self.pending_work.lock().unwrap() = Some(work);
    }
}
fn new_run(timeout: Duration) -> Run {
    Run::new(RunSpec::main().with_timeout(timeout).unwrap(), None)
}

fn call(name: &str, input: serde_json::Value) -> ToolCall {
    ToolCall {
        id: sdk::ToolCallId::new_v7(),
        provider_id: format!("provider-{name}"),
        name: name.to_string(),
        index: 0,
        input,
    }
}

#[test]
fn input_split_keeps_user_content_and_controls_separate() {
    let batch = split_input_events(vec![
        ChatInputEvent::user_message("follow up", Vec::new()),
        ChatInputEvent::ControlCommand {
            raw: "/save".to_string(),
        },
        ChatInputEvent::Reset,
    ]);

    assert_eq!(batch.user_inputs.len(), 1);
    assert_eq!(batch.user_inputs[0].text, "follow up");
    assert_eq!(
        batch.controls,
        vec![
            RuntimeControl::Command("/save".to_string()),
            RuntimeControl::Reset,
        ]
    );
}

#[test]
fn stuck_guard_detects_repeated_text_for_every_run_kind() {
    for mut guard in [
        StuckGuard::new(Duration::ZERO),
        StuckGuard::new(Duration::from_secs(30)),
    ] {
        assert_eq!(guard.inspect_text("same"), StuckDecision::Allow);
        assert_eq!(guard.inspect_text("same"), StuckDecision::Allow);
        assert!(matches!(
            guard.inspect_text("same"),
            StuckDecision::SoftBlock { .. }
        ));
    }
}

#[test]
fn stuck_guard_detects_tool_loops_and_escalates() {
    let mut guard = StuckGuard::new(Duration::ZERO);
    let repeated = call("Read", json!({"file_path": "a.rs"}));

    assert_eq!(guard.inspect_tool(&repeated), StuckDecision::Allow);
    assert_eq!(guard.inspect_tool(&repeated), StuckDecision::Allow);
    assert!(matches!(
        guard.inspect_tool(&repeated),
        StuckDecision::SoftBlock { .. }
    ));
    let _ = guard.inspect_tool(&repeated);
    assert!(matches!(
        guard.inspect_tool(&repeated),
        StuckDecision::HardPause { .. }
    ));
}

#[test]
fn timeout_zero_is_unlimited_and_positive_timeout_fails() {
    let now = Instant::now();
    let unlimited = StuckGuard::with_started_at(Duration::ZERO, now);
    let finite = StuckGuard::with_started_at(Duration::from_secs(5), now);

    assert_eq!(
        unlimited.inspect_timeout(now + Duration::from_secs(60)),
        StuckDecision::Allow
    );
    assert!(matches!(
        finite.inspect_timeout(now + Duration::from_secs(5)),
        StuckDecision::Fail { .. }
    ));
}

// #1248 Task 6: Stop hook block counting moved to Run domain.
// The following test is removed because record_stop_hook_block no longer
// exists on StuckGuard. Equivalent coverage is in domain/agent_run/tests.rs
// and application/stop_hook_coordination_tests.rs.

#[tokio::test]
async fn engine_completes_text_only_run_through_the_run_fsm() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(port.frozen_steps.len(), 1);
    assert_eq!(port.finalized_steps, port.frozen_steps);
    assert_eq!(run.steps()[0].id(), &port.frozen_steps[0]);
    assert_eq!(run.steps().len(), 1);
    assert_eq!(
        run.steps()[0].invocation().unwrap().response(),
        "done",
        "the shared engine must record the model invocation in the Run aggregate"
    );
    assert_eq!(
        port.calls,
        vec![
            "emit",
            "input",
            "freeze_step",
            "accept_step_input",
            "emit",
            "needs_compaction",
            "model",
            "finalize_step",
            "input",
            "emit",
        ]
    );
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Completed { .. })));
}

#[tokio::test]
async fn engine_accepts_input_before_building_context() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    let accepted = port
        .calls
        .iter()
        .position(|call| *call == "accept_step_input")
        .unwrap();
    let context = port
        .calls
        .iter()
        .position(|call| *call == "needs_compaction")
        .unwrap();
    assert!(accepted < context);
}

#[tokio::test]
async fn engine_stops_before_context_when_accepted_input_durable_write_fails() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        fail_accept_input: true,
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert!(port.calls.contains(&"accept_step_input"));
    assert!(!port.calls.contains(&"needs_compaction"));
    assert!(!port.calls.contains(&"model"));
}

#[tokio::test]
async fn engine_executes_tools_then_reenters_the_same_loop() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "second".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(2),
            },
        ]),
        model_steps: VecDeque::from([
            ModelStep::Tools {
                text: "calling".to_string(),
                calls: vec![call("Read", json!({"file_path": "a.rs"}))],
            },
            ModelStep::Complete {
                text: "done".to_string(),
            },
        ]),
        tool_steps: VecDeque::from([ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(
        port.calls.iter().filter(|call| **call == "model").count(),
        2
    );
    assert_eq!(
        port.calls.iter().filter(|call| **call == "tools").count(),
        1
    );
    let first_step = &run.steps()[0];
    assert_eq!(first_step.tool_calls().len(), 1);
    assert_eq!(
        first_step.tool_calls()[0].status(),
        crate::domain::agent_run::ToolCallStatus::Success,
        "the shared engine must own the tool-call lifecycle"
    );
}

#[tokio::test]
async fn engine_pauses_for_user_without_completing_the_run() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);
}

#[tokio::test]
async fn provider_context_too_long_compacts_then_rebuilds_before_reinvoking() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        model_errors: VecDeque::from([LoopEngineError::NeedsCompaction(
            "provider context too long".to_string(),
        )]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(
        port.calls,
        vec![
            "emit",
            "input",
            "freeze_step",
            "accept_step_input",
            "emit",
            "needs_compaction",
            "model",
            "compact",
            "model",
            "finalize_step",
            "input",
            "emit",
        ]
    );
}

#[tokio::test]
async fn provider_context_too_long_after_compaction_fails_without_looping() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_errors: VecDeque::from([
            LoopEngineError::NeedsCompaction("first".to_string()),
            LoopEngineError::NeedsCompaction("second".to_string()),
        ]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert_eq!(
        port.calls.iter().filter(|call| **call == "compact").count(),
        1
    );
    assert_eq!(
        port.calls.iter().filter(|call| **call == "model").count(),
        2
    );
}

#[tokio::test]
async fn cancel_step_during_compaction_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        cancel_when_compact_starts: true,
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(port.calls.contains(&"compact"));
    assert!(!port.calls.contains(&"model"));
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::StepCancelled { .. })));
}
#[tokio::test]
async fn engine_cancels_in_flight_compaction_and_emits_terminal_ack() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_for_task.cancel();
    });

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Cancelled);
    assert!(port.calls.contains(&"compact"));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(!port.calls.contains(&"model"));
}

#[tokio::test]
async fn engine_cancels_in_flight_model_and_emits_terminal_ack() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        cancelled_during_model: true,
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_for_task.cancel();
    });

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Cancelled);
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::CancellationRequested { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
}

#[tokio::test]
async fn engine_passes_soft_block_decision_to_the_single_tool_adapter() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let repeated = call("Read", json!({"file_path": "a.rs"}));
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "one".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "two".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "three".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(2),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "four".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(3),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(4),
            },
        ]),
        model_steps: VecDeque::from([
            ModelStep::Tools {
                text: "one".to_string(),
                calls: vec![repeated.clone()],
            },
            ModelStep::Tools {
                text: "two".to_string(),
                calls: vec![repeated.clone()],
            },
            ModelStep::Tools {
                text: "three".to_string(),
                calls: vec![repeated],
            },
            ModelStep::Complete {
                text: "done".to_string(),
            },
        ]),
        tool_steps: VecDeque::from([ToolStep::Continue, ToolStep::Continue, ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(port.guarded_calls.len(), 3);
    assert_eq!(port.guarded_calls[0], vec![ToolGuardDecision::Allow]);
    assert_eq!(port.guarded_calls[1], vec![ToolGuardDecision::Allow]);
    assert!(matches!(
        port.guarded_calls[2].as_slice(),
        [ToolGuardDecision::SoftBlock { .. }]
    ));
}

#[tokio::test]
async fn engine_timeout_interrupts_a_blocked_model_call() {
    let mut run = new_run(Duration::from_millis(10));
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        block_model_forever: true,
        ..Default::default()
    };

    tokio::time::timeout(
        Duration::from_secs(1),
        run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &cancel,
            &mut port,
        ),
    )
    .await
    .expect("deadline must interrupt blocked model")
    .unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
}

#[tokio::test]
async fn awaiting_user_does_not_resume_without_input() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };
    assert_eq!(
        run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &cancel,
            &mut port
        )
        .await
        .unwrap(),
        LoopDirective::AwaitUser
    );
    let model_calls = port.calls.iter().filter(|call| **call == "model").count();

    assert_eq!(
        run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &cancel,
            &mut port
        )
        .await
        .unwrap(),
        LoopDirective::AwaitUser
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert_eq!(
        port.calls.iter().filter(|call| **call == "model").count(),
        model_calls
    );
}

#[tokio::test]
async fn failed_event_delivery_is_restored_to_the_run_outbox() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        fail_emit_once: true,
        ..Default::default()
    };

    let error = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, LoopEngineError::Adapter(_)));
    assert!(matches!(
        run.events(),
        [
            RunDomainEvent::Transitioned { .. },
            RunDomainEvent::Started { .. },
            RunDomainEvent::DrainingInput { .. }
        ]
    ));
}

#[tokio::test]
async fn engine_timeout_fails_before_starting_new_work() {
    let mut run = new_run(Duration::from_nanos(1));
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort::default();

    tokio::time::sleep(Duration::from_millis(1)).await;
    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert!(!port.calls.contains(&"model"));
}

// ── #1272 Drain outcome tests ──────────────────────────────────────────

/// InternalContinuation with ToolResults kind processes like user input
/// but uses DrainInternalContinuation transition (not DrainInputs).
#[tokio::test]
async fn engine_processes_internal_continuation() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::InternalContinuation {
                kind: InternalContinuationKind::ToolResults,
                batch: vec![],
                epoch: DrainEpoch(0),
            },
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "resumed".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    // drain_input + freeze + accept + compaction check + emit + model + finalize + emit
    assert!(port.calls.contains(&"freeze_step"));
    assert!(port.calls.contains(&"model"));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Completed { .. })));
}

/// #1272: InternalContinuation with empty batch while AwaitingUser
/// must NOT auto-resume. The engine returns AwaitUser;
/// only Ready (guaranteed non-empty) resumes from AwaitingUser.
#[tokio::test]
async fn internal_continuation_while_awaiting_user_without_input_stays_awaiting() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // First call: model → Tools → AwaitUser → EmptyAndSealed → AwaitUser
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    let calls_before_second_loop = port.calls.len();

    // Simulate: before user responds, a stop-hook fires.
    // The main adapter would produce InternalContinuation with empty batch.
    // Engine must stay AwaitingUser, not auto-resume.
    // #1272: after the first run_loop consumed Ready(epoch 0), the Run's
    // next_drain_epoch is 1 (EmptyAndSealed during AwaitingUser does NOT
    // advance epoch). InternalContinuation at epoch 1 will advance to 2.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::InternalContinuation {
            kind: InternalContinuationKind::StopHookFeedback {
                feedback: "stop hook".to_string(),
            },
            batch: vec![], // No user input yet
            epoch: DrainEpoch(1),
        },
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(
        directive,
        LoopDirective::AwaitUser,
        "InternalContinuation with empty batch must NOT resume from AwaitingUser"
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    // Only drain was called (no step processing). When AwaitingUser,
    // the engine calls await_user_input, which pushes "await_input".
    assert_eq!(
        port.calls.len(),
        calls_before_second_loop + 1,
        "Only one drain call should have been made, not step processing"
    );
    assert!(
        port.calls.last() == Some(&"await_input") || port.calls.last() == Some(&"input"),
        "Last call should be a drain call"
    );
}

/// #1272: InternalContinuation with user input while AwaitingUser
/// DOES resume — the batch carries the user's response.
#[tokio::test]
async fn internal_continuation_while_awaiting_user_with_input_resumes() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([
            ModelStep::Tools {
                text: "question".to_string(),
                calls: vec![call("AskUserQuestion", json!({}))],
            },
            ModelStep::Complete {
                text: "answered".to_string(),
            },
        ]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    let calls_before = port.calls.len();

    // User input arrives + stop hook fires simultaneously.
    // InternalContinuation carries the user input in batch.
    // #1272: after first run_loop, next_drain_epoch is 1 (EmptyAndSealed
    // during AwaitingUser does NOT advance epoch).
    // InternalContinuation at epoch 1 advances to epoch 2.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::InternalContinuation {
            kind: InternalContinuationKind::StopHookFeedback {
                feedback: "reminder".to_string(),
            },
            batch: vec![LoopInput {
                text: "yes".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            epoch: DrainEpoch(1),
        },
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(run.status(), RunStatus::Completed);
    // New calls were made (step frozen, model invoked, etc.)
    assert!(
        port.calls.len() > calls_before,
        "Should have made new calls after resuming"
    );
    assert!(port.calls.contains(&"freeze_step"));
    assert!(port.calls.contains(&"model"));
}

// ── #1272 terminal text persistence ──────────────────────────────────

/// The last assistant text before EmptyAndSealed MUST be carried in the
/// Completed event.  Previously `terminal_text` was reset to None at
/// the top of each loop iteration, so Complete→EmptyAndSealed lost it.
#[tokio::test]
async fn engine_completed_event_carries_last_assistant_text() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "final answer".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    // The Completed event must carry the assistant text from the model step.
    let completed = port
        .events
        .iter()
        .find_map(|event| match event {
            RunDomainEvent::Completed { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("Completed event must be emitted");
    assert_eq!(
        completed, "final answer",
        "Completed.result must contain the last assistant text"
    );
}

/// Multiple Complete→Continue→Complete steps: only the LAST assistant
/// text survives to the Completed event (not the first).
#[tokio::test]
async fn engine_terminal_text_is_the_last_assistant_text_not_the_first() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "second".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(2),
            },
        ]),
        model_steps: VecDeque::from([
            ModelStep::Continue {
                text: "not done yet".to_string(),
            },
            ModelStep::Complete {
                text: "now done".to_string(),
            },
        ]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    let completed = port
        .events
        .iter()
        .find_map(|event| match event {
            RunDomainEvent::Completed { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("Completed event must be emitted");
    assert_eq!(
        completed, "now done",
        "Completed.result must be the LAST assistant text, not the first"
    );
}

// ── #1272 epoch validation tests ─────────────────────────────────────

/// L1: The engine rejects a drain outcome with a wrong epoch.
/// The adapter must return the epoch the engine expects; mismatch
/// returns a Chinese-localized `LoopEngineError::Adapter`.
#[tokio::test]
async fn engine_rejects_wrong_epoch() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // Default drain_outcomes: Ready(epoch 0) then EmptyAndSealed(epoch 1).
    // This matches the engine's expected sequence: 0→1.
    // We override the first outcome to have epoch 5 — a clear mismatch.
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "test".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(5), // Engine expects 0
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(6),
            },
        ]),
        ..Default::default()
    };

    let err = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, LoopEngineError::Adapter(msg) if msg.contains("drain epoch 不匹配")),
        "Expected Chinese epoch mismatch error, got: {err:?}"
    );
}

// ── #1272 await_user_input epoch preservation tests ──────────────────

/// When AwaitingUser + NoInput, the engine must NOT advance the Run's
/// drain epoch. The buffer stays receptive and the next call uses the
/// same expected epoch.
#[tokio::test]
async fn await_user_input_empty_preserves_run_epoch() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // First call: Ready(epoch 0) → model → Tools → AwaitUser
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);

    // #1272: After EmptyAndSealed during AwaitingUser, the Run's drain
    // epoch must NOT have advanced past the Ready consumption.
    // Ready(epoch 0) advanced to 1; EmptyAndSealed during AwaitingUser
    // did NOT advance. So next_drain_epoch is 1 (NOT 2).
    assert_eq!(
        run.next_drain_epoch(),
        1,
        "epoch must NOT advance for EmptyAndSealed during AwaitingUser"
    );
}

/// Same Run: AwaitUser → empty drain (NoInput) → AwaitUser → then user
/// input arrives at the same epoch → re-enter with correct epoch, consume
/// input, complete the Run. Epoch must be continuous with no jump.
#[tokio::test]
async fn await_user_input_empty_then_input_same_epoch_reenter() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            // This EmptyAndSealed will be consumed during AwaitingUser
            // (the legacy path for ScriptedPort). Epoch stays at 1.
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    // First run_loop: consumes Ready(0), executes step → AwaitUser,
    // then consumes EmptyAndSealed(1) during AwaitingUser → returns AwaitUser.
    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.next_drain_epoch(), 1);

    // Simulate: user input arrives. Next drain should work at epoch 1.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::ready(
            vec![LoopInput {
                text: "user response".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(1),
        ),
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);
    port.model_steps = VecDeque::from([ModelStep::Complete {
        text: "final answer".to_string(),
    }]);

    // Re-enter: same epoch (1), user input consumed, run completes.
    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Completed);
    // Epoch advanced: Ready(1) → 2, EmptyAndSealed(2) → 3
    assert_eq!(run.next_drain_epoch(), 3);
}

/// When the engine receives a wrong epoch from drain_input (not
/// AwaitingUser), the Run's drain epoch must NOT be advanced because
/// the error path returns before `advance_drain_epoch`.
#[tokio::test]
async fn drain_input_epoch_mismatch_does_not_advance_run_epoch() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            // This outcome has epoch 5 but the port's drain_epoch starts at 0
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "wrong-epoch-input".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(5),
            ),
        ]),
        ..Default::default()
    };

    let epoch_before = run.next_drain_epoch();
    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await;
    assert!(result.is_err(), "should return epoch mismatch error");
    // The Run's drain epoch must NOT have advanced
    assert_eq!(
        run.next_drain_epoch(),
        epoch_before,
        "epoch must NOT advance on drain_input error"
    );
}

// ── #1272 close-out: empty Ready + default await_user_input tests ─────

/// `DrainOutcome::ready(vec![])` must NOT panic — the assert has been
/// removed and empty-batch detection lives in `run_loop`.
#[test]
fn drain_outcome_ready_empty_does_not_panic() {
    // If this panics, the test itself fails.
    let outcome = DrainOutcome::ready(vec![], DrainEpoch(0));
    match &outcome {
        DrainOutcome::Ready { batch, .. } => assert!(batch.is_empty()),
        _ => panic!("expected Ready variant, got {outcome:?}"),
    }
}

/// When `run_loop` receives an empty `Ready` batch from the adapter, it
/// must return `Err(Adapter)` WITHOUT advancing epoch, transitioning state,
/// or calling `freeze_step` / `invoke_model`.
#[tokio::test]
async fn run_loop_empty_ready_returns_err_without_executing_step() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        // First (and only) drain returns an empty Ready batch.
        drain_outcomes: VecDeque::from([DrainOutcome::Ready {
            batch: vec![],
            epoch: DrainEpoch(0),
        }]),
        // Provide a model step that should NEVER be invoked.
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "should-not-run".to_string(),
        }]),
        ..Default::default()
    };

    let epoch_before = run.next_drain_epoch();
    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await;

    let err = result.expect_err("empty Ready must produce an error");
    assert!(
        matches!(&err, LoopEngineError::Adapter(msg) if msg.contains("空的 Ready batch")),
        "Expected Chinese empty-Ready Adapter error, got: {err:?}"
    );

    // Epoch must NOT have advanced.
    assert_eq!(
        run.next_drain_epoch(),
        epoch_before,
        "epoch must NOT advance for empty Ready"
    );
    // Run must NOT be terminal (no Completed/Failed transition).
    assert!(
        !run.status().is_terminal(),
        "Run must not be terminal after empty Ready error"
    );
    // freeze_step / model must NOT have been called.
    assert!(
        !port.calls.contains(&"freeze_step"),
        "freeze_step must not be called for empty Ready"
    );
    assert!(
        !port.calls.contains(&"model"),
        "invoke_model must not be called for empty Ready"
    );
}

// ── DrainOnlyPort: implements only drain_input (no await_user_input) ──

/// A minimal port that implements `drain_input` but does NOT override
/// `await_user_input`, relying on the trait default. Used to verify that
/// entering `AwaitingUser` with the default impl returns an Adapter error
/// instead of delegating to `drain_input` (which could seal the buffer).
struct DrainOnlyPort {
    drain_outcomes: VecDeque<DrainOutcome>,
    drain_epoch: DrainEpoch,
    model_steps: VecDeque<ModelStep>,
    tool_steps: VecDeque<ToolStep>,
    events: Vec<RunDomainEvent>,
    calls: Vec<&'static str>,
}

#[async_trait::async_trait]
impl InputPort for DrainOnlyPort {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.calls.push("drain_input");
        if expected_epoch != self.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.drain_epoch,
            )));
        }
        let outcome =
            self.drain_outcomes
                .pop_front()
                .unwrap_or_else(|| DrainOutcome::EmptyAndSealed {
                    epoch: self.drain_epoch,
                });
        self.drain_epoch = self.drain_epoch.next();
        Ok(outcome)
    }
}

#[async_trait::async_trait]
impl EventSinkPort for DrainOnlyPort {
    async fn emit(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        self.events.extend(events);
        Ok(())
    }
}

impl InteractionCompletionPort for DrainOnlyPort {
    fn interaction_execution_scope(&self) -> tools::ExecutionScope {
        unreachable!("DrainOnlyPort does not complete interactions")
    }

    fn interaction_tool_execution(&self) -> &dyn tools::ToolExecutionPort {
        unreachable!("DrainOnlyPort does not complete interactions")
    }

    fn interaction_materializer(
        &self,
    ) -> &crate::application::tool::result_materialization::ToolResultMaterializer {
        unreachable!("DrainOnlyPort does not complete interactions")
    }

    fn interaction_session_id(&self) -> &str {
        "drain-only"
    }

    fn interaction_cancellation(
        &self,
        _step_cancel: CancellationToken,
    ) -> std::sync::Arc<dyn tools::CancellationSignal> {
        unreachable!("DrainOnlyPort does not complete interactions")
    }
}

impl InteractionMailboxPort for DrainOnlyPort {}

impl StepPersistencePort for DrainOnlyPort {}

impl RunControlPort for DrainOnlyPort {
    fn take_control(&self, _run_id: &sdk::RunId) -> Option<RunControl> {
        None
    }
}

impl RunLifecyclePort for DrainOnlyPort {
    fn claim_terminal(&self, _run_id: &sdk::RunId) -> bool {
        true
    }

    fn claim_cancellation(&self, _run_id: &sdk::RunId) -> bool {
        true
    }

    fn register_step_scope(
        &self,
        _run_id: &sdk::RunId,
        _step_id: sdk::RunStepId,
        _cancel: CancellationToken,
    ) {
    }
}

#[async_trait::async_trait]
impl CompactionPort for DrainOnlyPort {
    async fn needs_compaction(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        Ok(false)
    }

    async fn compact(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _cancel: &CancellationToken,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl ModelInvocationPort for DrainOnlyPort {
    async fn invoke_model(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        self.calls.push("model");
        self.model_steps
            .pop_front()
            .map(|step| (step, StepTokenUsage::default()))
            .ok_or_else(|| LoopEngineError::Adapter("missing model step".to_string()))
    }
}

#[async_trait::async_trait]
impl StopHookPort for DrainOnlyPort {}

#[async_trait::async_trait]
impl ToolOrchestrationPort for DrainOnlyPort {
    async fn execute_tools(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
        _calls: &[(ToolCall, ToolGuardDecision)],
        _cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        self.calls.push("tools");
        self.tool_steps
            .pop_front()
            .map(
                |step| crate::application::tool::coordination::ToolRoundOutcome {
                    continuation: if matches!(
                        step,
                        ToolStep::Continue | ToolStep::ContinueWithFuseBypass(_)
                    ) {
                        crate::application::tool::coordination::ToolRoundContinuation::ToolResults
                    } else {
                        crate::application::tool::coordination::ToolRoundContinuation::None
                    },
                    step,
                },
            )
            .ok_or_else(|| LoopEngineError::Adapter("missing tool step".to_string()))
    }
}

#[async_trait::async_trait]
impl StuckHandlingPort for DrainOnlyPort {
    async fn on_stuck(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        _decision: &StuckDecision,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

impl PlanApprovalPort for DrainOnlyPort {}

/// A port that only implements `drain_input` (no `await_user_input` override)
/// must receive a Chinese Adapter error when the Run enters `AwaitingUser`,
/// NOT a silent delegation to `drain_input` (which would seal the buffer).
#[tokio::test]
async fn default_await_user_input_returns_error_not_delegating_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = DrainOnlyPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            // This would be consumed by drain_input if the default impl
            // delegated — but it should NOT be reached.
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        drain_epoch: DrainEpoch(0),
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        events: Vec::new(),
        calls: Vec::new(),
    };

    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut port,
    )
    .await;
    let err = result.expect_err("default await_user_input must return Err");

    assert!(
        matches!(&err, LoopEngineError::Adapter(msg)
            if msg.contains("未覆写 await_user_input")),
        "Expected Chinese 'not overridden' Adapter error, got: {err:?}"
    );

    // drain_input was called exactly once (for the first Ready).
    // It must NOT have been called a second time when AwaitingUser
    // triggered await_user_input.
    let drain_count = port.calls.iter().filter(|&&c| c == "drain_input").count();
    assert_eq!(
        drain_count, 1,
        "drain_input must be called exactly once (first Ready), \
         NOT delegated to by await_user_input"
    );
    // Run must be in AwaitingUser (the step produced AwaitUser before
    // the error interrupted the loop).
    assert_eq!(run.status(), RunStatus::AwaitingUser);
}

// ── #1247 typed Run control scenario tests ─────────────────────────────

#[tokio::test]
async fn terminate_run_during_compaction_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        terminate_when_compact_starts: true,
        ..Default::default()
    };

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port.calls.contains(&"compact"));
    assert!(!port.calls.contains(&"model"));
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Terminated { .. })));
}

#[tokio::test]
async fn cancel_step_during_model_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        cancel_when_model_starts: true,
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "should-not-complete".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(port.calls.contains(&"model"));
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::StepCancelled { .. })));
}

#[tokio::test]
async fn cancel_step_during_tools_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        cancel_when_tools_starts: true,
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "calling".to_string(),
            calls: vec![call("Read", json!({"file_path": "a.rs"}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut port,
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(port.calls.contains(&"tools"));
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::StepCancelled { .. })));
}

#[tokio::test]
async fn terminate_while_awaiting_user_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    // First run_loop: enters AwaitingUser.
    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);

    // AwaitUser 前的 step outcome 必须已被 finalize（持久化）。
    // 否则 Terminate 时 active_step 为 None，step 的模型回复会永久丢失。
    assert_eq!(
        port.finalized_steps.len(),
        1,
        "AwaitUser 前的 step 必须已 finalize，否则 Terminate 时 outcome 丢失"
    );

    // Inject TerminateRun control; root cancel fires so drain is interrupted.
    port.controls
        .lock()
        .unwrap()
        .push_back(RunControl::Terminate {
            reason: sdk::RunTerminationReason::SessionShutdown,
            deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_789),
        });
    root.cancel();

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut port,
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Terminated { .. })));
}

// ═══════════════════════════════════════════════════════════════════
// #1248 Task 5: Four-body interaction routing engine tests (RED)
// ═══════════════════════════════════════════════════════════════════

mod interaction_routing {
    use super::*;
    use crate::application::loop_engine::{
        ApprovalRequiredCall, SuspendedQuestion, SuspendedToolCall,
    };

    /// Helper: Create a port+run with Tools model step and given tool_step.
    fn setup_tool_run(
        model_step: ModelStep,
        tool_step: ToolStep,
    ) -> (Run, CancellationToken, ScriptedPort) {
        let run = Run::new(RunSpec::main(), None);
        let root = CancellationToken::new();
        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "user input".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let port = ScriptedPort {
            model_steps: VecDeque::from([model_step]),
            tool_steps: VecDeque::from([tool_step]),
            drain_outcomes: drain_q,
            ..Default::default()
        };
        (run, root, port)
    }

    // ── UserQuestions: InteractionSuspended → engine creates intent ──

    /// InteractionSuspended registers via coordinator, publishes to UI,
    /// stores receiver, and returns AwaitUser.
    #[tokio::test]
    async fn user_questions_suspension_creates_awaiting_user() {
        let call = call("AskUserQuestion", json!({"question": "continue?"}));
        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "continue?".to_string(),
                options: vec!["yes".to_string(), "no".to_string()],
                allow_multi: false,
            }],
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            },
            ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut port,
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);
        assert!(run.pending_interaction().is_some());
        assert!(
            port.calls.contains(&"publish_interaction"),
            "should have published: {:?}",
            port.calls
        );
    }

    // ── Continuation identity ──

    /// InteractionSuspended preserves CompleteToolCall continuation with
    /// the call ID from the suspended tool call.
    #[tokio::test]
    async fn interaction_suspended_preserves_continuation_identity() {
        let call_id = sdk::ids::ToolCallId::from_legacy_or_new("my-call-id");
        let call = ToolCall {
            id: call_id.clone(),
            provider_id: "provider-1".to_string(),
            name: "AskUserQuestion".to_string(),
            index: 0,
            input: json!({"question": "q"}),
        };

        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q".to_string(),
                options: vec!["a".to_string()],
                allow_multi: false,
            }],
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            },
            ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut port,
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);

        let pending = run
            .pending_interaction()
            .expect("should have pending interaction");
        assert_eq!(
            pending.continuation,
            InteractionContinuation::CompleteToolCall(call_id)
        );

        // Verify published interaction has UserQuestions body
        let published = port.published_interactions.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert!(matches!(
            published[0].body,
            sdk::InteractionRequestBody::UserQuestions(_)
        ));
    }

    // ── L2: ToolApproval: AwaitingToolApproval → coordinator ──

    /// AwaitingToolApproval creates ToolApproval intent via coordinator,
    /// stores the receiver, and returns AwaitUser.
    #[tokio::test]
    async fn tool_approval_creates_awaiting_user() {
        let call = call("Bash", json!({"command": "ls"}));
        let call_id = call.id.clone();
        let approval = ApprovalRequiredCall {
            call: call.clone(),
            authorization: tools::AuthorizationContext::STANDARD,
            reason: "approval required: high risk".to_string(),
            subject: "exec".to_string(),
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            },
            ToolStep::AwaitingToolApproval {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                calls_needing_approval: vec![approval],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut port,
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert!(run.pending_interaction().is_some());
        let pending = run.pending_interaction().unwrap();
        assert_eq!(
            pending.continuation,
            InteractionContinuation::ContinueToolApproval(call_id)
        );
    }

    // ── Multi-suspension: serial two AskUserQuestion ──

    /// Two AskUserQuestion calls: only the first is started immediately;
    /// the second is queued via PendingInteractionWork.
    #[tokio::test]
    async fn multi_suspension_queues_second_and_does_not_complete_step() {
        let call1 = call("AskUserQuestion", json!({"question": "q1"}));
        let call2 = call("AskUserQuestion", json!({"question": "q2"}));
        let suspended1 = SuspendedToolCall {
            call: call1.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q1".to_string(),
                options: vec!["a".to_string()],
                allow_multi: false,
            }],
        };
        let suspended2 = SuspendedToolCall {
            call: call2.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q2".to_string(),
                options: vec!["b".to_string()],
                allow_multi: false,
            }],
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call1, call2],
            },
            ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended1, suspended2],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut port,
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert!(run.pending_interaction().is_some());
        // Only one interaction was started; the second is queued on the port
        let pending_work = port.pending_work.lock().unwrap();
        assert!(
            pending_work.is_some(),
            "second suspension should be queued via set_pending_interaction_work"
        );
        let work = pending_work.as_ref().unwrap();
        assert_eq!(work.queue.len(), 1, "one item should be in the queue");
        assert!(
            work.active_step_id.is_some(),
            "active step should be preserved"
        );
    }

    // ── RequireApproval: full engine roundtrip approve ──

    /// Full engine roundtrip for tool approval: setup fake_tool_port,
    /// first run_loop → AwaitUser, reply approve via bridge,
    /// second run_loop → tool executes → Success.
    #[tokio::test]
    async fn require_approval_approve_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();
        let fake = Arc::new(FakeToolExecutionPort::new());
        fake.set_result_text("approved result");

        let call = call("Bash", json!({"command": "ls"}));
        let call_id = call.id.clone();

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "run ls".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedPort {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::AwaitingToolApproval {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                calls_needing_approval: vec![ApprovalRequiredCall {
                    call: call.clone(),
                    authorization: tools::AuthorizationContext::STANDARD,
                    reason: "dangerous".to_string(),
                    subject: "exec".to_string(),
                }],
            }]),
            drain_outcomes: drain_q,
            fake_tool_port: Some(fake.clone()),
            ..Default::default()
        };

        // First run_loop: engine creates ToolApproval intent → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Get the request_id from execution-owned mailbox metadata
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();

        // Reply approve via the interaction bridge
        let reply = sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve);
        let outcome = port.interaction_bridge.reply(&request_id, reply);
        assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);

        // Set up drain outcomes for second run_loop: complete after resolution
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);

        // Second run_loop: polls resolved interaction, finishes work, completes
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Assertions: tool executed once with correct invocation
        assert_eq!(fake.execute_count(), 1);
        let invocations = fake.recorded_invocations.lock().unwrap();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].tool_name.as_str(), "Bash");
        assert_eq!(invocations[0].input, json!({"command": "ls"}));
        assert_eq!(
            invocations[0].authorization,
            tools::AuthorizationContext::STANDARD
        );
        // set_result_text was used — verify the returned text
        assert_eq!(fake.returned_text(), Some("approved result".to_string()));

        // Verify the tool call has Success status in the Run
        let step = &run.steps()[0];
        assert_eq!(step.tool_calls().len(), 1);
        assert_eq!(step.tool_calls()[0].status(), ToolCallStatus::Success);
        assert_eq!(step.tool_calls()[0].id(), &call_id);
    }

    // ── RequireApproval: full engine roundtrip deny ──

    /// Full engine roundtrip for tool approval deny:
    /// first run_loop → AwaitUser, reply deny via bridge,
    /// second run_loop → tool NOT executed, Cancelled.
    #[tokio::test]
    async fn require_approval_deny_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();
        let fake = Arc::new(FakeToolExecutionPort::new());

        let call = call("Bash", json!({"command": "rm -rf /"}));
        let call_id = call.id.clone();

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "dangerous cmd".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedPort {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::AwaitingToolApproval {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                calls_needing_approval: vec![ApprovalRequiredCall {
                    call: call.clone(),
                    authorization: tools::AuthorizationContext::STANDARD,
                    reason: "dangerous".to_string(),
                    subject: "destroy".to_string(),
                }],
            }]),
            drain_outcomes: drain_q,
            fake_tool_port: Some(fake.clone()),
            ..Default::default()
        };

        // First run_loop → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);

        // Reply deny via the interaction bridge
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply =
            sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Deny { reason: None });
        let outcome = port.interaction_bridge.reply(&request_id, reply);
        assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);

        // Second run_loop: resolve → Cancelled
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Assertions: tool was NOT executed
        assert_eq!(fake.execute_count(), 0);

        // Tool call status is Cancelled
        let step = &run.steps()[0];
        assert_eq!(step.tool_calls().len(), 1);
        assert_eq!(step.tool_calls()[0].status(), ToolCallStatus::Cancelled);
        assert_eq!(step.tool_calls()[0].id(), &call_id);
    }

    // ── UserQuestions: single question full roundtrip ──

    /// Full engine roundtrip for a single UserQuestions interaction:
    /// run_loop → AwaitUser, reply via bridge, re-enter → Success.
    #[tokio::test]
    async fn user_questions_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();

        let call = call("AskUserQuestion", json!({"question": "continue?"}));
        let call_id = call.id.clone();
        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "continue?".to_string(),
                options: vec!["yes".to_string(), "no".to_string()],
                allow_multi: false,
            }],
        };

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "ask question".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedPort {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            }]),
            drain_outcomes: drain_q,
            ..Default::default()
        };

        // First run_loop → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Reply via bridge
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("yes".to_string())]);
        let outcome = port.interaction_bridge.reply(&request_id, reply);
        assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);

        // Second run_loop: resolve → Success
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Tool call is Success
        let step = &run.steps()[0];
        assert_eq!(step.tool_calls().len(), 1);
        assert_eq!(step.tool_calls()[0].status(), ToolCallStatus::Success);
        assert_eq!(step.tool_calls()[0].id(), &call_id);
    }

    // ── UserQuestions: two questions serial roundtrip ──

    /// Two AskUserQuestion calls: first resolved → second becomes active,
    /// second resolved → step completes. No direct finish seam.
    #[tokio::test]
    async fn user_questions_two_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();

        let call1 = call("AskUserQuestion", json!({"question": "q1"}));
        let call2 = call("AskUserQuestion", json!({"question": "q2"}));
        let call1_id = call1.id.clone();
        let call2_id = call2.id.clone();

        let suspended1 = SuspendedToolCall {
            call: call1.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q1".to_string(),
                options: vec!["a".to_string()],
                allow_multi: false,
            }],
        };
        let suspended2 = SuspendedToolCall {
            call: call2.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q2".to_string(),
                options: vec!["b".to_string()],
                allow_multi: false,
            }],
        };

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "ask two".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedPort {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call1.clone(), call2.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended1, suspended2],
            }]),
            drain_outcomes: drain_q,
            ..Default::default()
        };

        // First run_loop: first question active, second queued → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Reply to first question via bridge
        let request_id1 = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply1 = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("a".to_string())]);
        assert_eq!(
            port.interaction_bridge.reply(&request_id1, reply1),
            sdk::InteractionCommandOutcome::Accepted
        );

        // Second run_loop: resolve first, start second → AwaitUser again
        port.drain_outcomes = VecDeque::from([DrainOutcome::NoInput {
            epoch: DrainEpoch(1),
        }]);
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // First call now Success, step still active (second interaction pending)
        let step = &run.steps()[0];
        let tc1 = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &call1_id)
            .unwrap();
        assert_eq!(tc1.status(), ToolCallStatus::Success);
        assert!(
            run.active_step_id().is_some(),
            "step should still be active while second interaction is pending"
        );

        // Reply to second question via bridge
        let request_id2 = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply2 = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("b".to_string())]);
        assert_eq!(
            port.interaction_bridge.reply(&request_id2, reply2),
            sdk::InteractionCommandOutcome::Accepted
        );

        // Third run_loop: resolve second, complete step → terminal
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Both calls are Success, step is completed
        let step = &run.steps()[0];
        let tc1 = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &call1_id)
            .unwrap();
        let tc2 = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &call2_id)
            .unwrap();
        assert_eq!(tc1.status(), ToolCallStatus::Success);
        assert_eq!(tc2.status(), ToolCallStatus::Success);
        assert!(
            run.active_step_id().is_none(),
            "step should be completed after all interactions resolve"
        );
    }

    // ── Mixed: completed_results + suspended roundtrip ──

    /// Mixed round: one non-interaction call + one suspended question.
    /// First run_loop: non-interaction already Success, suspension creates AwaitUser.
    /// After reply: suspension becomes Success, non-interaction is NOT re-advanced.
    #[tokio::test]
    async fn mixed_completed_and_suspension_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();

        let bash_call = call("Bash", json!({"command": "ls"}));
        let question_call = call("AskUserQuestion", json!({"question": "go?"}));
        let bash_id = bash_call.id.clone();
        let question_id = question_call.id.clone();

        let suspended_q = SuspendedToolCall {
            call: question_call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "go?".to_string(),
                options: vec!["yes".to_string()],
                allow_multi: false,
            }],
        };

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "mixed".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedPort {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![bash_call.clone(), question_call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: vec![(bash_id.clone(), ToolCallStatus::Success)],
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended_q],
            }]),
            drain_outcomes: drain_q,
            ..Default::default()
        };

        // First run_loop: non-interaction → Success, suspension → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Bash call already Success (advanced by engine before interaction)
        let step = &run.steps()[0];
        let bash_tc = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &bash_id)
            .unwrap();
        assert_eq!(
            bash_tc.status(),
            ToolCallStatus::Success,
            "non-interaction call should be Success after first run"
        );

        // Reply to the question via bridge
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("yes".to_string())]);
        assert_eq!(
            port.interaction_bridge.reply(&request_id, reply),
            sdk::InteractionCommandOutcome::Accepted
        );

        // Second run_loop: resolve suspension → complete step
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        let directive = run_loop(&mut run, &mut execution, &cancel, &mut port)
            .await
            .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Both calls are Success; bash was NOT re-advanced
        let step = &run.steps()[0];
        let bash_tc = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &bash_id)
            .unwrap();
        let question_tc = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &question_id)
            .unwrap();
        assert_eq!(bash_tc.status(), ToolCallStatus::Success);
        assert_eq!(question_tc.status(), ToolCallStatus::Success);
    }
}
