use super::*;
use crate::application::interaction::port::{InteractionBridge, InteractionPort};
use crate::application::tool::agent::ToolCall;
use sdk::InteractionRequest;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn engine_sources() -> String {
    [
        include_str!("engine.rs"),
        include_str!("engine/contracts.rs"),
        include_str!("engine/phases.rs"),
        include_str!("engine/step_driver.rs"),
        include_str!("engine/interaction_driver.rs"),
        include_str!("engine/control_driver.rs"),
    ]
    .join("\n")
}

#[test]
fn execution_state_is_owned_by_engine_and_not_exposed_as_a_port() {
    let engine_source = engine_sources();
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
fn domain_events_are_observed_by_activity_before_external_publish() {
    let source = include_str!("run_loop.rs");

    assert!(source.contains(".observe_run_events(&events)"));
    assert!(source.contains("self.events.emit(execution, events).await"));
    assert!(
        source
            .find(".observe_run_events(&events)")
            .expect("activity observation")
            < source
                .find("self.events.emit(execution, events).await")
                .expect("external event publish")
    );
    let engine_source = engine_sources();
    assert!(engine_source.contains("run.restore_events(events)"));
}

#[test]
fn engine_state_transitions_use_the_immediate_publish_boundary() {
    let source = engine_sources();
    let direct_transition_count = source.matches("run.transition(RunTransition::").count();

    assert_eq!(
        direct_transition_count, 0,
        "Engine state changes must go through transition_and_emit; found {direct_transition_count} direct transitions",
    );
    assert!(source.contains("async fn transition_and_emit("));
    assert!(source.contains(
        "transition_and_emit(run, execution, port, RunTransition::ContextPrepared).await?"
    ));
}

#[test]
fn production_engine_owns_step_state_reset() {
    let source = engine_sources();
    assert!(source.contains("execution.begin_step()"));
    assert!(source.contains("run_loop(run, execution, cancel, loop_context)"));

    let main_source = include_str!("../loop_engine/chat/session_driver/run_launch.rs");
    let sub_source = include_str!("../run/derived/loop_run.rs");
    assert!(main_source.contains("crate::application::run::launcher::launch("));
    assert!(sub_source.contains("crate::application::run::launcher::launch("));
    assert!(!main_source.contains("loop_engine::run_loop("));
    assert!(!sub_source.contains("loop_engine::run_loop("));
}

#[test]
fn production_engine_entry_uses_supplied_execution_and_context() {
    let source = engine_sources();
    assert!(!source.contains("let _ = (execution, context)"));
    assert!(source.contains("run_loop(run, execution, cancel, loop_context)"));
    assert!(source.contains("context.event_sink()") || source.contains("context.input()"));
}

use crate::application::loop_engine::{
    CompactProgressView, DrainEpoch, DrainOutcome, InternalContinuationKind, LoopInput,
};

#[test]
fn p6_9_7_runtime_tests_use_independent_capability_fakes() {
    let source = [
        include_str!("engine_architecture_tests.rs"),
        include_str!("engine_scenarios_tests.rs"),
        include_str!("engine_control_tests.rs"),
        include_str!("engine_input_tests.rs"),
        include_str!("engine_activity_tests.rs"),
    ]
    .join("\n");
    let source = source
        .split_once("#[derive(Default)]\nstruct ScriptedObservations")
        .map(|(_, fixtures)| fixtures)
        .expect("test fixture marker must exist");

    for forbidden in [
        "impl InputPort for ScriptedScenario",
        "impl EventSinkPort for ScriptedScenario",
        "impl RunControlPort for ScriptedScenario",
        "impl ModelInvocationPort for ScriptedScenario",
        "struct DrainOnlyPort",
        "impl EventSinkPort for DrainOnlyPort",
        "impl ModelInvocationPort for DrainOnlyPort",
        "unsafe {\n        RunLoop::new(",
    ] {
        assert!(
            !source.contains(forbidden),
            "Runtime test retains fat capability fake: {forbidden}"
        );
    }

    assert!(source.contains("struct ScriptedScenario"));
    assert!(source.contains("struct ScriptedPorts"));
    assert!(source.contains("struct DrainInputFake"));
}

#[test]
fn p6_9_engine_has_no_fat_capability_aggregation_trait() {
    let engine = engine_sources();
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
        "MainRunCapabilities",
        "SubRunCapabilities",
        "MainEventStrategy",
        "SubEventStrategy",
        "SubAgentEventSink",
    ] {
        assert!(
            !main.contains(retired) && !sub.contains(retired) && !sub_setup.contains(retired),
            "retired role-shaped execution shell remains: {retired}"
        );
    }
    assert!(sub.contains("async fn launch_sub_run("));
}

#[test]
fn p6_9_4_source_directories_expose_only_source_observer_topology_and_mapping() {
    let chat = include_str!("../loop_engine/chat/main_run_port.rs");
    let derived = include_str!("../run/derived/loop_run.rs");

    let chat_topology = include_str!("../loop_engine/chat/session_driver/run_launch.rs");
    let derived_topology = include_str!("../run/derived/setup.rs");

    for (name, source) in [("Chat", chat), ("Derived", derived)] {
        for forbidden in [
            "async fn invoke_model_impl(",
            "provider.invoke(",
            "ModelInvocationCoordinator::new()",
            "async fn execute_tools_impl(",
            "prepare_tool_round(",
            "execute_tool_round(",
            "HookInvocation::Stop",
            "ContextRequest {",
            "async fn finalize_sub_agent(",
            "impl crate::application::loop_engine::ModelInvocationPort",
            "impl crate::application::loop_engine::ToolOrchestrationPort",
            "impl crate::application::loop_engine::CompactionPort",
            "impl crate::application::interaction::coordinator::InteractionCompletionContextProvider",
            "impl crate::application::loop_engine::StepPersistencePort",
            "impl crate::application::loop_engine::RunLifecyclePort",
            "RuntimeStepPersistence::new(",
            "RuntimeCompaction::new(",
            "RuntimeInteraction::new(",
            "RuntimeModelInvocation::new(",
            "RuntimeStopHook::new(",
            "RuntimeToolOrchestration::new(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} source directory retains workflow ownership: {forbidden}"
            );
        }
    }

    assert!(chat.contains("BufferedInputAdapter"));
    assert!(chat.contains("ChatStreamEventObserver"));
    assert!(derived.contains("ProgressTerminalObserver"));
    for topology in [chat_topology, derived_topology] {
        assert!(topology.contains("RuntimeStepPersistence::new("));
        assert!(topology.contains("RuntimeCompaction::new("));
        assert!(topology.contains("RuntimeInteraction::new("));
        assert!(topology.contains("RuntimeModelInvocation::new("));
        assert!(topology.contains("RuntimeStopHook::new("));
        assert!(topology.contains("RuntimeToolOrchestration::new("));
    }
}

#[test]
fn p6_9_6_engine_expresses_context_model_and_tool_as_typed_narrow_phases() {
    let engine = engine_sources();

    for required in [
        "enum InputDrainOutcome",
        "async fn run_input_drain_phase<P>(",
        "enum StepInputOutcome",
        "async fn run_step_input_phase<P>(",
        "enum StepFinalizationOutcome",
        "async fn run_step_finalization_phase<P>(",
        "enum ContextCompactionOutcome",
        "async fn run_context_compaction_phase<P>(",
        "enum ModelInvocationOutcome",
        "async fn run_model_invocation_phase<P>(",
        "enum ToolRoundPhaseOutcome",
        "async fn run_tool_round_phase<P>(",
    ] {
        assert!(
            engine.contains(required),
            "Engine is missing typed narrow phase: {required}"
        );
    }

    assert!(engine.contains("P: InputPort + ?Sized"));
    assert!(engine.contains("P: StepPersistencePort + ?Sized"));
    assert!(engine.contains("P: CompactionPort + ?Sized"));
    assert!(engine.contains("P: ModelInvocationPort + ?Sized"));
    assert!(engine.contains("P: ToolOrchestrationPort + ?Sized"));
}

#[test]
fn p6_9_7_engine_entry_has_no_fat_loop_adapter() {
    let engine = engine_sources();
    let launcher = include_str!("../run/launcher.rs");
    let main = include_str!("../loop_engine/chat/main_run_port.rs");
    let derived = include_str!("../run/derived/loop_run.rs");

    for retired in [
        "LoopCapabilityAdapter",
        "ChatLoopCapabilityAdapter",
        "DerivedLoopCapabilityAdapter",
    ] {
        assert!(
            !engine.contains(retired)
                && !launcher.contains(retired)
                && !main.contains(retired)
                && !derived.contains(retired),
            "fat Loop adapter remains in production: {retired}"
        );
    }
    for fat_bound in [
        "P: InputPort\n        + EventSinkPort",
        "P: crate::application::loop_engine::InputPort\n        + crate::application::loop_engine::EventSinkPort",
    ] {
        assert!(
            !engine.contains(fat_bound) && !launcher.contains(fat_bound),
            "fat Loop adapter was mechanically expanded into one generic capability intersection: {fat_bound}"
        );
    }
    assert!(engine.contains("pub async fn execute_prepared_loop("));
    assert!(launcher.contains("pub async fn launch("));
}

#[test]
fn stop_hook_has_one_application_owner_and_narrow_observer() {
    let engine = engine_sources();
    let coordinator = include_str!("../hook/stop_coordination.rs");
    let services = include_str!("run_services.rs");
    let main = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub = include_str!("../run/derived/loop_run.rs");

    assert!(coordinator.contains("pub struct StopHookExecutionContext"));
    assert!(coordinator.contains("pub trait StopHookObserver"));
    assert!(coordinator.contains("pub async fn coordinate_stop_hook"));
    assert!(coordinator.contains("execution.record_step_message"));
    assert!(coordinator.contains("execution.append_message"));

    assert!(!engine.contains("pub trait StopHookPort"));
    assert!(services.contains("impl<O> StopHookObserver for RuntimeStopHook<O>"));
    assert!(main.contains("ChatStopHookObserver"));
    assert!(main.contains("stop_coordination::StopHookObserver"));
    assert!(sub.contains("NoopStopHookObserver"));
    for (name, observer) in [("Main", main), ("Sub", sub)] {
        for retired in [
            "impl crate::application::loop_engine::StopHookPort",
            "fn stop_hook_context",
            "fn project_stop_hook_outcome",
        ] {
            assert!(
                !observer.contains(retired),
                "{name} retains role-specific Stop Hook orchestration seam: {retired}"
            );
        }
    }
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
    let engine = engine_sources();
    let main_adapter = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub_adapter = include_str!("../run/derived/loop_run.rs");

    assert!(engine.contains("struct StepCommit"));
    assert!(engine.contains("fn prepare_step_commit"));
    assert!(engine.contains("duration_ms: execution"));
    assert!(engine.contains(".step_elapsed()"));
    let persistence = include_str!("step_persistence.rs");
    assert!(persistence.contains("commit.duration_ms"));
    for adapter in [main_adapter, sub_adapter] {
        assert!(!adapter.contains("committed_message_count() + execution.accepted_input_len()"));
        assert!(!adapter.contains("execution.commit_all_messages()"));
        assert!(!adapter.contains("execution.commit_step_messages()"));
    }
}

#[test]
fn p6_9_3_shared_run_services_delegate_to_role_neutral_owners() {
    let services = include_str!("run_services.rs");
    let main_topology = include_str!("../loop_engine/chat/session_driver/run_launch.rs");
    let sub_topology = include_str!("../run/derived/setup.rs");

    for owner in [
        "ContextRequestCoordinator::new(",
        "StepPersistenceCoordinator::from_context(",
        "CompactionCoordinator::from_context(",
        "InteractionCompletionContext::new(",
        "orchestrate_model_invocation(",
        "ToolRoundCoordinator::new(",
    ] {
        assert!(
            services.contains(owner),
            "共享 Run 服务必须委托职责明确的 application owner：{owner}"
        );
    }

    for topology in [main_topology, sub_topology] {
        for service in [
            "RuntimeStepPersistence::new(",
            "RuntimeCompaction::new(",
            "RuntimeInteraction::new(",
            "RuntimeModelInvocation::new(",
            "RuntimeStopHook::new(",
            "RuntimeToolOrchestration::new(",
        ] {
            assert!(
                topology.contains(service),
                "Main/Derived topology 必须装配同一职责型窄服务：{service}"
            );
        }
    }

    assert!(!services.contains("LoopCapabilityAdapter"));
    assert!(!services.contains("ChatLoopCapabilityAdapter"));
    assert!(!services.contains("DerivedLoopCapabilityAdapter"));
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
        context: &tools::ToolExecutionContext,
    ) -> tools::ToolExecutionOutcome {
        let _ = context;
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

#[derive(Default)]
struct ScriptedObservations {
    calls: Vec<&'static str>,
    events: Vec<RunDomainEvent>,
    guarded_calls: Vec<Vec<ToolGuardDecision>>,
    cancelled_steps: Vec<sdk::RunStepId>,
    finalized_steps: Vec<sdk::RunStepId>,
    frozen_steps: Vec<sdk::RunStepId>,
}

struct ScriptedState {
    model_steps: VecDeque<ModelStep>,
    model_errors: VecDeque<LoopEngineError>,
    tool_steps: VecDeque<ToolStep>,
    registered_step: Option<sdk::RunStepId>,
    cancel_when_compact_starts: bool,
    cancel_when_model_starts: bool,
    cancel_when_tools_starts: bool,
    terminate_when_compact_starts: bool,
    cancelled_during_model: bool,
    model_started: Arc<std::sync::Barrier>,
    active_run: Option<Arc<dyn crate::domain::agent_run::ActiveRunPort>>,
    require_model_cancellation_cleanup: bool,
    model_cancellation_cleanup_completed: bool,
    block_await_user_input_forever: bool,
    block_compact_until_cancelled: bool,
    fail_accept_input: bool,
    needs_compaction: bool,
    fail_emit_once: bool,
    drain_outcomes: VecDeque<DrainOutcome>,
    drain_epoch: DrainEpoch,
    observations: ScriptedObservations,}

impl Default for ScriptedState {
    fn default() -> Self {
        Self {
            model_steps: VecDeque::new(),
            model_errors: VecDeque::new(),
            tool_steps: VecDeque::new(),
            registered_step: None,
            cancel_when_compact_starts: false,
            cancel_when_model_starts: false,
            cancel_when_tools_starts: false,
            terminate_when_compact_starts: false,
            cancelled_during_model: false,
            model_started: Arc::new(std::sync::Barrier::new(1)),
            active_run: None,
            require_model_cancellation_cleanup: false,
            model_cancellation_cleanup_completed: false,
            block_await_user_input_forever: false,
            block_compact_until_cancelled: false,
            fail_accept_input: false,
            needs_compaction: false,
            fail_emit_once: false,
            drain_outcomes: VecDeque::new(),
            drain_epoch: DrainEpoch(0),
            observations: ScriptedObservations::default(),
        }
    }}

#[derive(Clone)]
struct InputFake(Arc<std::sync::Mutex<ScriptedState>>);
#[derive(Clone)]
struct EventSinkFake(Arc<std::sync::Mutex<ScriptedState>>);
#[derive(Clone)]
struct RunControlFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
    controls: Arc<std::sync::Mutex<VecDeque<RunControl>>>,
}
#[derive(Clone)]
struct RunLifecycleFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
    step_cancel: Arc<std::sync::Mutex<Option<CancellationToken>>>,
}
#[derive(Clone)]
struct StepPersistenceFake(Arc<std::sync::Mutex<ScriptedState>>);
#[derive(Clone)]
struct CompactionFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
    controls: Arc<std::sync::Mutex<VecDeque<RunControl>>>,
}
#[derive(Clone)]
struct ModelInvocationFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
    controls: Arc<std::sync::Mutex<VecDeque<RunControl>>>,
}
#[derive(Clone)]
struct ToolOrchestrationFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
    controls: Arc<std::sync::Mutex<VecDeque<RunControl>>>,
}
#[derive(Clone)]
struct StuckHandlingFake(Arc<std::sync::Mutex<ScriptedState>>);
struct StopHookFake;
struct PlanApprovalFake;
struct InteractionMailboxFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
    interaction_bridge: Arc<InteractionBridge>,
    published_interactions: Arc<std::sync::Mutex<Vec<InteractionRequest>>>,
    pending_work: Arc<std::sync::Mutex<Option<super::engine::PendingInteractionWork>>>,
    fake_tool_port: Option<Arc<FakeToolExecutionPort>>,
}

struct ScriptedPorts {
    input: InputFake,
    events: EventSinkFake,
    control: RunControlFake,
    lifecycle: RunLifecycleFake,
    interaction: InteractionMailboxFake,
    persistence: StepPersistenceFake,
    compaction: CompactionFake,
    model: ModelInvocationFake,
    stop_hook: StopHookFake,
    tools: ToolOrchestrationFake,
    stuck: StuckHandlingFake,
    plan_approval: PlanApprovalFake,
}

pub(crate) struct ScenarioLoopHarness {
    scenario: ScriptedScenario,
}

impl ScenarioLoopHarness {
    pub(crate) fn completes_with(text: impl Into<String>) -> Self {
        Self {
            scenario: ScriptedScenario {
                model_steps: VecDeque::from([ModelStep::Complete { text: text.into() }]),
                ..Default::default()
            },
        }
    }

    pub(crate) fn blocks_in_model() -> Self {
        Self {
            scenario: ScriptedScenario {
                cancelled_during_model: true,
                model_started: Arc::new(std::sync::Barrier::new(2)),
                ..Default::default()
            },
        }
    }

    pub(crate) fn model_started(&self) -> Arc<std::sync::Barrier> {
        Arc::clone(&self.scenario.model_started)
    }

    pub(crate) fn use_active_run_control(
        &mut self,
        active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    ) {
        self.scenario.active_run = Some(active_run);
    }

    pub(crate) fn run_loop(&mut self) -> RunLoop<'_> {
        scripted_run_loop(&mut self.scenario)
    }

    pub(crate) fn saw_input_and_model(&self) -> bool {
        let calls = self.scenario.calls();
        calls.contains(&"input") && calls.contains(&"model")
    }

    pub(crate) fn terminal_event_count(&self) -> usize {
        self.completed_terminal_event_count()
    }

    pub(crate) fn completed_terminal_event_count(&self) -> usize {
        self.scenario
            .events()
            .iter()
            .filter(|event| matches!(event, RunDomainEvent::Completed { .. }))
            .count()
    }

    pub(crate) fn cancelled_step_count(&self) -> usize {
        self.scenario.cancelled_steps().len()
    }
}

struct ScriptedScenario {
    model_steps: VecDeque<ModelStep>,
    model_errors: VecDeque<LoopEngineError>,
    tool_steps: VecDeque<ToolStep>,
    controls: Arc<std::sync::Mutex<VecDeque<RunControl>>>,
    cancel_when_compact_starts: bool,
    cancel_when_model_starts: bool,
    cancel_when_tools_starts: bool,
    terminate_when_compact_starts: bool,
    step_cancel: Arc<std::sync::Mutex<Option<CancellationToken>>>,
    drain_outcomes: VecDeque<DrainOutcome>,
    drain_epoch: DrainEpoch,
    cancelled_during_model: bool,
    model_started: Arc<std::sync::Barrier>,
    active_run: Option<Arc<dyn crate::domain::agent_run::ActiveRunPort>>,
    require_model_cancellation_cleanup: bool,
    model_cancellation_cleanup_completed: bool,
    block_await_user_input_forever: bool,
    block_compact_until_cancelled: bool,
    fail_accept_input: bool,
    needs_compaction: bool,
    fail_emit_once: bool,
    interaction_bridge: Arc<InteractionBridge>,
    published_interactions: Arc<std::sync::Mutex<Vec<InteractionRequest>>>,
    pending_work: Arc<std::sync::Mutex<Option<super::engine::PendingInteractionWork>>>,
    fake_tool_port: Option<Arc<FakeToolExecutionPort>>,
    state: Arc<std::sync::Mutex<ScriptedState>>,
    ports: Option<ScriptedPorts>,
}

impl Default for ScriptedScenario {
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
            model_steps: VecDeque::new(),
            model_errors: VecDeque::new(),
            tool_steps: VecDeque::new(),
            controls: Arc::new(std::sync::Mutex::new(VecDeque::new())),
            cancel_when_compact_starts: false,
            cancel_when_model_starts: false,
            cancel_when_tools_starts: false,
            terminate_when_compact_starts: false,
            step_cancel: Arc::new(std::sync::Mutex::new(None)),
            drain_outcomes,
            drain_epoch: DrainEpoch(0),
            cancelled_during_model: false,
            model_started: Arc::new(std::sync::Barrier::new(1)),
            active_run: None,
            require_model_cancellation_cleanup: false,
            model_cancellation_cleanup_completed: false,
            block_await_user_input_forever: false,
            block_compact_until_cancelled: false,
            fail_accept_input: false,
            needs_compaction: false,
            fail_emit_once: false,
            interaction_bridge: Arc::new(InteractionBridge::new()),
            published_interactions: Arc::new(std::sync::Mutex::new(Vec::new())),
            pending_work: Arc::new(std::sync::Mutex::new(None)),
            fake_tool_port: None,
            state: Arc::new(std::sync::Mutex::new(ScriptedState::default())),
            ports: None,
        }
    }}

impl ScriptedScenario {
    fn ports(&mut self) -> &mut ScriptedPorts {
        if self.ports.is_none() {
            let state = ScriptedState {
                model_steps: std::mem::take(&mut self.model_steps),
                model_errors: std::mem::take(&mut self.model_errors),
                tool_steps: std::mem::take(&mut self.tool_steps),
                cancel_when_compact_starts: self.cancel_when_compact_starts,
                cancel_when_model_starts: self.cancel_when_model_starts,
                cancel_when_tools_starts: self.cancel_when_tools_starts,
                terminate_when_compact_starts: self.terminate_when_compact_starts,
                cancelled_during_model: self.cancelled_during_model,
                model_started: Arc::clone(&self.model_started),
                active_run: self.active_run.clone(),
                require_model_cancellation_cleanup: self.require_model_cancellation_cleanup,
                model_cancellation_cleanup_completed: self.model_cancellation_cleanup_completed,
                block_await_user_input_forever: self.block_await_user_input_forever,
                block_compact_until_cancelled: self.block_compact_until_cancelled,
                fail_accept_input: self.fail_accept_input,
                needs_compaction: self.needs_compaction,
                fail_emit_once: self.fail_emit_once,
                drain_outcomes: std::mem::take(&mut self.drain_outcomes),
                drain_epoch: self.drain_epoch,
                ..ScriptedState::default()
            };
            self.state = Arc::new(std::sync::Mutex::new(state));
            let state = Arc::clone(&self.state);
            let controls = Arc::clone(&self.controls);
            self.ports = Some(ScriptedPorts {                input: InputFake(Arc::clone(&state)),
                events: EventSinkFake(Arc::clone(&state)),
                control: RunControlFake {
                    state: Arc::clone(&state),
                    controls: Arc::clone(&controls),
                },
                lifecycle: RunLifecycleFake {
                    state: Arc::clone(&state),
                    step_cancel: Arc::clone(&self.step_cancel),
                },
                interaction: InteractionMailboxFake {
                    state: Arc::clone(&state),
                    interaction_bridge: Arc::clone(&self.interaction_bridge),
                    published_interactions: Arc::clone(&self.published_interactions),
                    pending_work: Arc::clone(&self.pending_work),
                    fake_tool_port: self.fake_tool_port.clone(),
                },
                persistence: StepPersistenceFake(Arc::clone(&state)),
                compaction: CompactionFake {
                    state: Arc::clone(&state),
                    controls: Arc::clone(&controls),
                },
                model: ModelInvocationFake {
                    state: Arc::clone(&state),
                    controls: Arc::clone(&controls),
                },
                stop_hook: StopHookFake,
                tools: ToolOrchestrationFake {
                    state: Arc::clone(&state),
                    controls,
                },
                stuck: StuckHandlingFake(state),
                plan_approval: PlanApprovalFake,
            });
        }
        self.ports.as_mut().expect("scripted ports must exist")
    }

    fn sync_inputs(&mut self) {
        let mut state = self.state.lock().unwrap();
        if !self.drain_outcomes.is_empty() {
            state.drain_outcomes = std::mem::take(&mut self.drain_outcomes);
        }
        if !self.model_steps.is_empty() {
            state.model_steps = std::mem::take(&mut self.model_steps);
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.state.lock().unwrap().observations.calls.clone()
    }

    fn events(&self) -> Vec<RunDomainEvent> {
        self.state.lock().unwrap().observations.events.clone()
    }

    fn guarded_calls(&self) -> Vec<Vec<ToolGuardDecision>> {
        self.state
            .lock()
            .unwrap()
            .observations
            .guarded_calls
            .clone()
    }

    fn cancelled_steps(&self) -> Vec<sdk::RunStepId> {
        self.state
            .lock()
            .unwrap()
            .observations
            .cancelled_steps
            .clone()
    }

    fn finalized_steps(&self) -> Vec<sdk::RunStepId> {
        self.state
            .lock()
            .unwrap()
            .observations
            .finalized_steps
            .clone()
    }

    fn frozen_steps(&self) -> Vec<sdk::RunStepId> {
        self.state.lock().unwrap().observations.frozen_steps.clone()
    }
}

impl ScriptedPorts {
    fn run_loop(&mut self) -> RunLoop<'_> {
        RunLoop::new(
            &mut self.input,
            &mut self.events,
            &self.control,
            &self.lifecycle,
            &mut self.interaction,
            &mut self.persistence,
            &mut self.compaction,
            &mut self.model,
            &mut self.stop_hook,
            &mut self.tools,
            &mut self.stuck,
            &self.plan_approval,
        )
    }
}

#[async_trait::async_trait]
impl InputPort for InputFake {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        let mut state = self.0.lock().unwrap();
        state.observations.calls.push("input");
        if expected_epoch != state.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, state.drain_epoch,
            )));
        }
        let epoch = state.drain_epoch;
        let outcome = state
            .drain_outcomes
            .pop_front()
            .unwrap_or(DrainOutcome::EmptyAndSealed { epoch });
        if !matches!(&outcome, DrainOutcome::NoInput { .. }) {
            state.drain_epoch = state.drain_epoch.next();
        }
        Ok(outcome)
    }

    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        let block_forever = {
            let mut state = self.0.lock().unwrap();
            state.observations.calls.push("await_input");
            if expected_epoch != state.drain_epoch {
                return Err(LoopEngineError::Adapter(format!(
                    "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                    expected_epoch, state.drain_epoch,
                )));
            }
            state.block_await_user_input_forever
        };
        if block_forever {
            std::future::pending::<()>().await;
            unreachable!("pending future must not complete");
        }
        let mut state = self.0.lock().unwrap();
        let epoch = state.drain_epoch;
        let outcome = state
            .drain_outcomes
            .pop_front()
            .unwrap_or(DrainOutcome::NoInput { epoch });
        if !matches!(
            &outcome,
            DrainOutcome::EmptyAndSealed { .. } | DrainOutcome::NoInput { .. }
        ) {
            state.drain_epoch = state.drain_epoch.next();
        }
        Ok(outcome)
    }
}

#[async_trait::async_trait]
impl EventSinkPort for EventSinkFake {
    async fn emit(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        let mut state = self.0.lock().unwrap();
        state.observations.calls.push("emit");
        if state.fail_emit_once {
            state.fail_emit_once = false;
            return Err(LoopEngineError::Adapter("emit failed".to_string()));
        }
        state.observations.events.extend(events);
        Ok(())
    }
}

#[async_trait::async_trait]
impl RunControlPort for RunControlFake {
    fn take_control(&self, run_id: &sdk::RunId) -> Option<RunControl> {
        let active_run = self.state.lock().unwrap().active_run.clone();
        active_run
            .and_then(|active_run| active_run.take_control(run_id))
            .or_else(|| self.controls.lock().unwrap().pop_front())
    }
}

#[async_trait::async_trait]
impl RunLifecyclePort for RunLifecycleFake {
    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        let active_run = {
            let mut state = self.state.lock().unwrap();
            state.registered_step = Some(step_id.clone());
            state.active_run.clone()
        };
        *self.step_cancel.lock().unwrap() = Some(cancel.clone());
        if let Some(active_run) = active_run {
            active_run.set_main_active_step(run_id, step_id, cancel);
        }
    }

    fn clear_step_scope(&self, run_id: &sdk::RunId, step_id: &sdk::RunStepId) {
        let active_run = self.state.lock().unwrap().active_run.clone();
        if let Some(active_run) = active_run {
            active_run.clear_main_active_step(run_id, step_id);
        }
    }
}

#[async_trait::async_trait]
impl StepPersistencePort for StepPersistenceFake {
    fn observe_step_frozen(&mut self, step_id: &sdk::RunStepId) {
        let mut state = self.0.lock().unwrap();
        state.observations.calls.push("freeze_step");
        state.observations.frozen_steps.push(step_id.clone());
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
        let mut state = self.0.lock().unwrap();
        state.observations.calls.push("accept_step_input");
        if state.fail_accept_input {
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
        let mut state = self.0.lock().unwrap();
        match commit.cause {
            crate::ports::FinalizeCause::Completed => {
                state.observations.calls.push("finalize_step");
                state
                    .observations
                    .finalized_steps
                    .push(commit.step_id.clone());
            }
            crate::ports::FinalizeCause::UserCancelledStep
            | crate::ports::FinalizeCause::RunTerminated => {
                state.observations.calls.push("finalize_cancelled_step");
                state
                    .observations
                    .cancelled_steps
                    .push(commit.step_id.clone());
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CompactionPort for CompactionFake {
    async fn needs_compaction(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        let mut state = self.state.lock().unwrap();
        state.observations.calls.push("needs_compaction");
        Ok(state.needs_compaction)
    }

    async fn compact(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &CancellationToken,
        _progress: std::sync::Arc<dyn CompactProgressView>,
    ) -> Result<(), LoopEngineError> {
        let (registered_step, cancel_step, terminate_run, block_until_cancelled) = {
            let mut state = self.state.lock().unwrap();
            state.observations.calls.push("compact");
            (
                state.registered_step.clone(),
                state.cancel_when_compact_starts,
                state.terminate_when_compact_starts,
                state.block_compact_until_cancelled,
            )
        };
        if cancel_step {
            let step_id = registered_step.expect("step scope must be registered before compact");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
        }
        if terminate_run {
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::Terminate {
                    reason: sdk::RunTerminationReason::UserExit,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_456),
                });
            cancel.cancel();
        }
        if block_until_cancelled {
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ModelInvocationPort for ModelInvocationFake {
    async fn invoke_model(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _step_id: &sdk::RunStepId,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        let (
            registered_step,
            cancel_model,
            cancelled_during_model,
            require_cancellation_cleanup,
            error,
        ) = {
            let mut state = self.state.lock().unwrap();
            state.observations.calls.push("model");
            (
                state.registered_step.clone(),
                state.cancel_when_model_starts,
                state.cancelled_during_model,
                state.require_model_cancellation_cleanup,
                state.model_errors.pop_front(),
            )
        };
        if cancel_model {
            let step_id = registered_step.expect("step scope must be registered before model");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
            if !require_cancellation_cleanup {
                return Err(LoopEngineError::Cancelled);
            }
        }
        if require_cancellation_cleanup {
            cancel.cancelled().await;
            self.state
                .lock()
                .unwrap()
                .model_cancellation_cleanup_completed = true;
            return Err(LoopEngineError::Cancelled);
        }
        if cancelled_during_model {
            let model_started = Arc::clone(&self.state.lock().unwrap().model_started);
            model_started.wait();
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        if let Some(error) = error {
            return Err(error);
        }
        self.state            .lock()
            .unwrap()
            .model_steps
            .pop_front()
            .map(|step| (step, StepTokenUsage::default()))
            .ok_or_else(|| LoopEngineError::Adapter("missing model step".to_string()))
    }
}

#[async_trait::async_trait]
impl crate::application::hook::stop_coordination::StopHookObserver for StopHookFake {}

#[async_trait::async_trait]
impl ToolOrchestrationPort for ToolOrchestrationFake {
    async fn finalize_streaming_tool_results(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _step_id: &sdk::RunStepId,
        rounds: Vec<
            crate::application::loop_engine::chat::streaming_tool::StreamingToolRoundResult,
        >,
        _cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        let mut results = Vec::new();
        let mut suspensions = Vec::new();
        let mut approvals = Vec::new();
        let mut fuse_bypassed = Vec::new();
        for round in rounds {
            results.extend(round.results);
            suspensions.extend(round.suspensions);
            approvals.extend(round.approvals);
            fuse_bypassed.extend(round.fuse_bypassed);
        }
        if !suspensions.is_empty() {
            return Ok(crate::application::tool::coordination::ToolRoundOutcome {
                step: ToolStep::InteractionSuspended {
                    suspended: suspensions,
                    completed_results: Vec::new(),
                    fuse_bypassed,
                },
                continuation: crate::application::tool::coordination::ToolRoundContinuation::None,
            });
        }
        if !approvals.is_empty() {
            return Ok(crate::application::tool::coordination::ToolRoundOutcome {
                step: ToolStep::AwaitingToolApproval {
                    calls_needing_approval: approvals,
                    completed_results: Vec::new(),
                    fuse_bypassed,
                },
                continuation: crate::application::tool::coordination::ToolRoundContinuation::None,
            });
        }
        Ok(crate::application::tool::coordination::ToolRoundOutcome {
            step: ToolStep::Continue,
            continuation:
                crate::application::tool::coordination::ToolRoundContinuation::ToolResults,
        })
    }

    async fn execute_tools(
        &mut self,
        _execution: &mut crate::application::run::execution_state::RunExecutionState,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        let (registered_step, cancel_tools, step) = {
            let mut state = self.state.lock().unwrap();
            state.observations.calls.push("tools");
            state
                .observations
                .guarded_calls
                .push(calls.iter().map(|(_, decision)| decision.clone()).collect());
            (
                state.registered_step.clone(),
                state.cancel_when_tools_starts,
                state.tool_steps.pop_front(),
            )
        };
        if cancel_tools {
            let step_id = registered_step.expect("step scope must be registered before tools");
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
        step.map(
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
impl StuckHandlingPort for StuckHandlingFake {
    async fn on_stuck(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        _decision: &StuckDecision,
    ) -> Result<(), LoopEngineError> {
        self.0.lock().unwrap().observations.calls.push("stuck");
        Ok(())
    }
}

impl PlanApprovalPort for PlanApprovalFake {}

impl crate::application::interaction::coordinator::InteractionCompletionContextProvider
    for InteractionMailboxFake
{
    fn interaction_completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> crate::application::interaction::coordinator::InteractionCompletionContext<'_> {
        static MATERIALIZER: std::sync::OnceLock<
            std::sync::Arc<
                crate::application::tool::tool_result_materializer::ToolResultMaterializer,
            >,
        > = std::sync::OnceLock::new();
        let materializer = MATERIALIZER
            .get_or_init(crate::application::tool::test_support::test_tool_result_materializer)
            .as_ref();
        static UNUSED_TOOL_EXECUTION: std::sync::LazyLock<FakeToolExecutionPort> =
            std::sync::LazyLock::new(FakeToolExecutionPort::new);
        let tool_execution = self.fake_tool_port.as_deref().map_or(
            &*UNUSED_TOOL_EXECUTION as &dyn tools::ToolExecutionPort,
            |port| port,
        );
        let tool_context =
            crate::application::run::workspace_test_support::test_tool_execution_context(
                std::path::PathBuf::from("/tmp"),
                step_cancel,
            );
        crate::application::interaction::coordinator::InteractionCompletionContext::new(
            tool_context,
            tool_execution,
            materializer,
            "test-session",
        )
    }
}

#[async_trait::async_trait]
impl InteractionMailboxPort for InteractionMailboxFake {
    fn interaction_port(&self) -> &dyn InteractionPort {
        self.interaction_bridge.as_ref()
    }

    async fn publish_interaction(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        request: &InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        self.state
            .lock()
            .unwrap()
            .observations
            .calls
            .push("publish_interaction");
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
        self.state
            .lock()
            .unwrap()
            .observations
            .calls
            .push("set_pending_interaction_work");
        execution.set_pending_interaction_work(work.clone());
        *self.pending_work.lock().unwrap() = Some(work);
    }
}

fn scripted_run_loop(scenario: &mut ScriptedScenario) -> RunLoop<'_> {
    let mut run_loop = scenario.ports().run_loop();
    run_loop.bind_test_activity_context();
    run_loop
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
