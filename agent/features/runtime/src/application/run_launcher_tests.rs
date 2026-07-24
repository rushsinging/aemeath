use super::*;
use crate::application::active_run::ActiveRunRegistry;
use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, LoopEngineError, ModelStep, RunLoopPort, StepTokenUsage,
    StuckDecision, ToolGuardDecision, ToolStep,
};
use crate::domain::agent_run::{ActiveRunPort, RunDomainEvent, RunId, RunSpec, RunStepId};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Minimal RunLoopPort stub. It either seals immediately or fails while
/// draining, and records every domain event published by the launcher.
struct StubPort {
    drain_error: Option<String>,
    events_emitted: Vec<RunDomainEvent>,
}

impl StubPort {
    fn completing() -> Self {
        Self {
            drain_error: None,
            events_emitted: Vec::new(),
        }
    }

    fn failing(error: impl Into<String>) -> Self {
        Self {
            drain_error: Some(error.into()),
            events_emitted: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl RunLoopPort for StubPort {
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

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        Ok(false)
    }

    async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        Ok(())
    }

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

    async fn execute_tools(
        &mut self,
        _run_id: &RunId,
        _step_id: &RunStepId,
        _calls: &[(crate::application::subagent::ToolCall, ToolGuardDecision)],
        _cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        Ok(ToolStep::Continue)
    }

    async fn on_stuck(&mut self, _decision: &StuckDecision) -> Result<(), LoopEngineError> {
        Ok(())
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        self.events_emitted.extend(events);
        Ok(())
    }
}

#[tokio::test]
async fn launch_creates_run_and_returns_terminal() {
    let registry: Arc<dyn ActiveRunPort> = Arc::new(ActiveRunRegistry::default());
    let mut port = StubPort::completing();
    let input = RunLaunchInput {
        run_id: RunId::new_v7(),
        spec: RunSpec::main(),
        parent_run_id: None,
        cancel: CancellationToken::new(),
    };

    let result = launch(input, registry.clone(), &mut port).await;
    assert!(matches!(result, RunLaunchResult::Terminal));
}

#[tokio::test]
async fn launch_clears_active_run_after_completion() {
    let registry = Arc::new(ActiveRunRegistry::default());
    let run_id = RunId::new_v7();
    let mut port = StubPort::completing();
    let input = RunLaunchInput {
        run_id: run_id.clone(),
        spec: RunSpec::main(),
        parent_run_id: None,
        cancel: CancellationToken::new(),
    };

    let _ = launch(input, registry.clone(), &mut port).await;

    assert!(!registry.claim_terminal(&run_id));
}

#[tokio::test]
async fn launch_adapter_error_emits_failed_terminal_and_clears_active_run() {
    let registry = Arc::new(ActiveRunRegistry::default());
    let run_id = RunId::new_v7();
    let mut port = StubPort::failing("compact skipped");

    let result = launch(
        RunLaunchInput {
            run_id: run_id.clone(),
            spec: RunSpec::main(),
            parent_run_id: None,
            cancel: CancellationToken::new(),
        },
        registry.clone(),
        &mut port,
    )
    .await;

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
