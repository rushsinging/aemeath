use std::sync::Arc;

use share::message::Message;

use crate::domain::session::{CommittedStepMessages, SessionHistory};
use crate::domain::{ContextMessages, ToolCallReceipt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectedRunPolicy {
    latest_complete_runs: usize,
}

impl ProtectedRunPolicy {
    pub const fn latest_complete_runs(latest_complete_runs: usize) -> Self {
        Self {
            latest_complete_runs,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextReadCandidate {
    runs: Arc<[ContextReadRun]>,
}

impl ContextReadCandidate {
    pub fn from_steps(
        runs: Vec<ContextReadRun>,
        active_run_id: &str,
        policy: ProtectedRunPolicy,
    ) -> Self {
        let complete_run_ids = runs
            .iter()
            .filter(|run| run.steps.iter().all(ContextReadStep::is_finalized))
            .map(|run| run.run_id.clone())
            .collect::<Vec<_>>();
        let protected_complete_run_ids = complete_run_ids
            .iter()
            .rev()
            .take(policy.latest_complete_runs)
            .cloned()
            .collect::<Vec<_>>();
        let runs = runs
            .into_iter()
            .map(|mut run| {
                let has_unfinalized_step = run.steps.iter().any(|step| !step.is_finalized());
                run.protected = run.run_id == active_run_id
                    || has_unfinalized_step
                    || protected_complete_run_ids.contains(&run.run_id);
                run
            })
            .collect();
        Self { runs }
    }

    pub fn from_history(
        history: &SessionHistory,
        active_run_id: &str,
        policy: ProtectedRunPolicy,
    ) -> Self {
        let runs = history
            .iter()
            .map(|run| ContextReadRun {
                run_id: run.run_id.clone(),
                protected: false,
                steps: run
                    .steps
                    .iter()
                    .map(|step| ContextReadStep {
                        step_id: step.step_id.clone(),
                        accepted_messages: step
                            .accepted_input
                            .as_ref()
                            .map(|input| input.messages.clone()),
                        outcome_messages: step
                            .outcome
                            .as_ref()
                            .map(|outcome| outcome.messages.clone()),
                        tool_receipts: step.tool_receipts.clone().into(),
                        finalized: step.outcome.is_some(),
                    })
                    .collect(),
            })
            .collect();
        Self::from_steps(runs, active_run_id, policy)
    }

    pub fn runs(&self) -> &[ContextReadRun] {
        &self.runs
    }

    pub fn map_unprotected_outcomes(
        &self,
        mut transform: impl FnMut(
            usize,
            usize,
            &ContextReadStep,
            &Arc<[Message]>,
        ) -> Option<Vec<Message>>,
    ) -> Self {
        let runs = self
            .runs
            .iter()
            .enumerate()
            .map(|(run_index, run)| {
                if run.protected {
                    return run.clone();
                }
                let steps = run
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(step_index, step)| {
                        let Some(messages) = step.outcome_messages.as_ref() else {
                            return step.clone();
                        };
                        let source = messages.as_arc();
                        let Some(transformed) = transform(run_index, step_index, step, &source)
                        else {
                            return step.clone();
                        };
                        let mut updated = step.clone();
                        updated.outcome_messages = Some(transformed.into());
                        updated
                    })
                    .collect();
                ContextReadRun {
                    run_id: run.run_id.clone(),
                    protected: run.protected,
                    steps,
                }
            })
            .collect();
        Self { runs }
    }

    pub fn run(&self, run_id: &str) -> Option<&ContextReadRun> {
        self.runs.iter().find(|run| run.run_id == run_id)
    }

    pub fn messages(&self) -> ContextMessages {
        let committed_steps = self
            .runs
            .iter()
            .flat_map(|run| run.steps.iter())
            .flat_map(|step| {
                step.accepted_messages
                    .iter()
                    .map(CommittedStepMessages::as_arc)
                    .chain(
                        step.outcome_messages
                            .iter()
                            .map(CommittedStepMessages::as_arc),
                    )
            })
            .collect();
        ContextMessages::from_committed_steps(committed_steps, Vec::new())
    }
}

#[derive(Debug, Clone)]
pub struct ContextReadRun {
    run_id: String,
    protected: bool,
    steps: Arc<[ContextReadStep]>,
}

impl ContextReadRun {
    pub fn new(run_id: impl Into<String>, steps: Vec<ContextReadStep>) -> Self {
        Self {
            run_id: run_id.into(),
            protected: false,
            steps: steps.into(),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub const fn is_protected(&self) -> bool {
        self.protected
    }

    pub fn steps(&self) -> &[ContextReadStep] {
        &self.steps
    }
}

#[derive(Debug, Clone)]
pub struct ContextReadStep {
    step_id: String,
    accepted_messages: Option<CommittedStepMessages>,
    outcome_messages: Option<CommittedStepMessages>,
    tool_receipts: Arc<[ToolCallReceipt]>,
    finalized: bool,
}

impl ContextReadStep {
    pub fn new(
        step_id: impl Into<String>,
        accepted_messages: Option<CommittedStepMessages>,
        outcome_messages: Option<CommittedStepMessages>,
        tool_receipts: Vec<ToolCallReceipt>,
        finalized: bool,
    ) -> Self {
        Self {
            step_id: step_id.into(),
            accepted_messages,
            outcome_messages,
            tool_receipts: tool_receipts.into(),
            finalized,
        }
    }

    pub fn outcome_messages(&self) -> Arc<[Message]> {
        self.outcome_messages
            .as_ref()
            .map(CommittedStepMessages::as_arc)
            .unwrap_or_default()
    }

    pub const fn is_finalized(&self) -> bool {
        self.finalized
    }

    pub fn step_id(&self) -> &str {
        &self.step_id
    }

    pub fn tool_receipts(&self) -> &[ToolCallReceipt] {
        &self.tool_receipts
    }
}
