use super::state::RunStepId;

pub use sdk::RunId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDomainEvent {
    Started {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    StepStarted {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
    },
    StepCompleted {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
    },
    CancellationRequested {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    AwaitingUser {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    Resumed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    StuckDetected {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        reason: String,
    },
    Completed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        result: String,
    },
    Failed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        error: String,
    },
    Cancelled {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
}

impl RunDomainEvent {
    pub fn parent_run_id(&self) -> Option<&RunId> {
        match self {
            Self::Started { parent_run_id, .. }
            | Self::StepStarted { parent_run_id, .. }
            | Self::StepCompleted { parent_run_id, .. }
            | Self::CancellationRequested { parent_run_id, .. }
            | Self::AwaitingUser { parent_run_id, .. }
            | Self::Resumed { parent_run_id, .. }
            | Self::StuckDetected { parent_run_id, .. }
            | Self::Completed { parent_run_id, .. }
            | Self::Failed { parent_run_id, .. }
            | Self::Cancelled { parent_run_id, .. } => parent_run_id.as_ref(),
        }
    }
}
