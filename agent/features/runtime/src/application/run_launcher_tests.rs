use super::*;
use crate::application::active_run::ActiveRunRegistry;
use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, LoopEngineError, LoopEnginePort, ModelStep, StepTokenUsage,
    StuckDecision, ToolGuardDecision, ToolStep,
};
use crate::domain::agent_run::{ActiveRunPort, RunDomainEvent, RunId, RunSpec, RunStepId};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Minimal LoopEnginePort stub. It either seals immediately or fails while
/// draining, and records every domain event published by the launcher.
struct StubPort {
    drain_error: Option<String>,
    events_emitted: Vec<RunDomainEvent>,
    execution: crate::application::run_execution_state::RunExecutionState,
}

impl StubPort {
    fn completing() -> Self {
        Self {
            drain_error: None,
            events_emitted: Vec::new(),
            execution: crate::application::run_execution_state::RunExecutionState::new(),
        }
    }

    fn failing(error: impl Into<String>) -> Self {
        Self {
            drain_error: Some(error.into()),
            events_emitted: Vec::new(),
            execution: crate::application::run_execution_state::RunExecutionState::new(),
        }
    }
}

#[async_trait::async_trait]
impl crate::application::loop_engine::InputPort for StubPort {
    async fn drain_input(
        &mut self,
        _expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        if let Some(error) = self.drain_error.take() {
            return Err(LoopEngineError::Adapter(error));
        }
        Ok(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(0),
        })
    }
}

#[async_trait::async_trait]
impl crate::application::loop_engine::EventSinkPort for StubPort {
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        self.events_emitted.extend(events);
        Ok(())
    }
}

impl crate::application::loop_engine::InteractionMailboxPort for StubPort {}

impl crate::application::loop_engine::StepPersistencePort for StubPort {}

impl crate::application::loop_engine::RunControlPort for StubPort {
    fn take_control(&self, _run_id: &RunId) -> Option<crate::domain::agent_run::RunControl> {
        None
    }
}

impl crate::application::loop_engine::RunLifecyclePort for StubPort {
    fn claim_terminal(&self, _run_id: &RunId) -> bool {
        true
    }

    fn claim_cancellation(&self, _run_id: &RunId) -> bool {
        true
    }

    fn register_step_scope(
        &self,
        _run_id: &RunId,
        _step_id: RunStepId,
        _cancel: CancellationToken,
    ) {
    }
}

#[async_trait::async_trait]
impl crate::application::loop_engine::CompactionPort for StubPort {
    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        Ok(false)
    }

    async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::application::loop_engine::ModelInvocationPort for StubPort {
    async fn invoke_model(
        &mut self,
        _cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        Ok((
            ModelStep::Complete {
                text: String::new(),
            },
            StepTokenUsage::default(),
        ))
    }
}

impl crate::application::loop_engine::StopHookPort for StubPort {}

#[async_trait::async_trait]
impl crate::application::loop_engine::ToolOrchestrationPort for StubPort {
    async fn execute_tools(
        &mut self,
        _run_id: &RunId,
        _step_id: &RunStepId,
        _calls: &[(crate::application::subagent::ToolCall, ToolGuardDecision)],
        _cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        Ok(ToolStep::Continue)
    }
}

#[async_trait::async_trait]
impl crate::application::loop_engine::StuckHandlingPort for StubPort {
    async fn on_stuck(&mut self, _decision: &StuckDecision) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

impl crate::application::loop_engine::PlanApprovalPort for StubPort {}

impl crate::application::loop_engine::ExecutionStatePort for StubPort {
    fn execution_state_mut(
        &mut self,
    ) -> &mut crate::application::run_execution_state::RunExecutionState {
        &mut self.execution
    }
}

#[async_trait::async_trait]
impl LoopEnginePort for StubPort {}

#[tokio::test]
async fn launch_creates_run_and_returns_terminal() {
    let registry: Arc<dyn ActiveRunPort> = Arc::new(ActiveRunRegistry::default());
    let mut port = StubPort::completing();
    let run = crate::domain::agent_run::Run::new(RunSpec::main(), None);
    let result = launch(run, CancellationToken::new(), registry.clone(), &mut port).await;
    assert!(matches!(result, RunLaunchResult::Terminal));
}

#[tokio::test]
async fn launch_clears_active_run_after_completion() {
    let registry = Arc::new(ActiveRunRegistry::default());
    let run_id = RunId::new_v7();
    let mut port = StubPort::completing();
    let run = crate::domain::agent_run::Run::with_id(run_id.clone(), RunSpec::main(), None);

    let _ = launch(run, CancellationToken::new(), registry.clone(), &mut port).await;

    assert!(!registry.claim_terminal(&run_id));
}

#[tokio::test]
async fn launch_adapter_error_emits_failed_terminal_and_clears_active_run() {
    let registry = Arc::new(ActiveRunRegistry::default());
    let run_id = RunId::new_v7();
    let mut port = StubPort::failing("compact skipped");

    let run = crate::domain::agent_run::Run::with_id(run_id.clone(), RunSpec::main(), None);
    let result = launch(run, CancellationToken::new(), registry.clone(), &mut port).await;

    assert!(matches!(
        result,
        RunLaunchResult::Failed(LoopEngineError::Adapter(ref error))
            if error == "compact skipped"
    ));
    let failures = port
        .events_emitted
        .iter()
        .filter_map(|event| match event {
            RunDomainEvent::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(failures, vec!["loop adapter error: compact skipped"]);
    assert!(!registry.claim_terminal(&run_id));
}
