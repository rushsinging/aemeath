use async_trait::async_trait;
use tools::AgentRunTerminal;

use crate::application::run::execution_state::RunExecutionState;
use crate::application::run::launcher::{RunLaunchResult, RunLaunchResult::Failed};

#[derive(Debug, Clone, PartialEq)]
pub enum RunFinalizationStatus {
    Completed,
    Cancelled,
    Failed(String),
    TimedOut,
    ApiError(String),
}

#[derive(Debug, Clone)]
pub struct RunFinalizationOutcome {
    pub status: RunFinalizationStatus,
    pub turns: usize,
    pub duration: std::time::Duration,
    pub role: Option<String>,
    pub model: String,
}

pub fn log_run_finalization(outcome: &RunFinalizationOutcome, session_id: &str) {
    log::info!(target: crate::LOG_TARGET,
        "[agent_loop_finished] session={}, status={:?}, turns={}, duration_ms={}, role={}, model={}",
        session_id,
        outcome.status,
        outcome.turns,
        outcome.duration.as_millis(),
        outcome.role.as_deref().unwrap_or("-"),
        outcome.model,
    );
}

#[async_trait]
pub(crate) trait RunFinalizationObserver: Send {
    async fn on_finalized(&mut self, outcome: &RunFinalizationOutcome, terminal: &AgentRunTerminal);
}

/// Role-neutral owner of terminal recovery, outcome classification, and final callback dispatch.
pub(crate) struct RunFinalizationCoordinator<O> {
    role: Option<String>,
    model: String,
    observer: O,
}

impl<O> RunFinalizationCoordinator<O>
where
    O: RunFinalizationObserver,
{
    pub(crate) fn new(role: Option<String>, model: impl Into<String>, observer: O) -> Self {
        Self {
            role,
            model: model.into(),
            observer,
        }
    }

    pub(crate) async fn finalize(
        mut self,
        execution: &mut RunExecutionState,
        launch_result: RunLaunchResult,
    ) -> AgentRunTerminal {
        let loop_error = match launch_result {
            RunLaunchResult::Terminal => None,
            Failed(error) => Some(error),
        };
        let terminal = execution
            .take_terminal()
            .unwrap_or_else(|| AgentRunTerminal::Failed {
                error: loop_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| {
                        "shared run loop ended without a terminal event".to_string()
                    }),
            });
        let status = match &terminal {
            AgentRunTerminal::Completed { .. } => RunFinalizationStatus::Completed,
            AgentRunTerminal::Failed { error } if error.starts_with("run timed out after ") => {
                RunFinalizationStatus::TimedOut
            }
            AgentRunTerminal::Failed { error } => RunFinalizationStatus::Failed(error.clone()),
            AgentRunTerminal::Cancelled => RunFinalizationStatus::Cancelled,
        };
        let outcome = RunFinalizationOutcome {
            status,
            turns: execution.turn_count(),
            duration: execution.elapsed(),
            role: self.role.clone(),
            model: self.model.clone(),
        };
        self.dispatch(outcome, &terminal).await;
        terminal
    }

    pub(crate) async fn finalize_terminal(
        mut self,
        status: RunFinalizationStatus,
        turns: usize,
        duration: std::time::Duration,
        terminal: &AgentRunTerminal,
    ) {
        let outcome = RunFinalizationOutcome {
            status,
            turns,
            duration,
            role: self.role.clone(),
            model: self.model.clone(),
        };
        self.dispatch(outcome, terminal).await;
    }

    async fn dispatch(&mut self, outcome: RunFinalizationOutcome, terminal: &AgentRunTerminal) {
        self.observer.on_finalized(&outcome, terminal).await;
    }
}
