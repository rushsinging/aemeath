use super::state::{RunStatus, RunStepId, RunTransitionReason};

pub use sdk::RunId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDomainEvent {
    Transitioned {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        from: RunStatus,
        to: RunStatus,
        reason: RunTransitionReason,
    },
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
    StepCancellationRequested {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
    },
    StepFinalizationStarted {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
    },
    StepCancelled {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
        confirmed: bool,
    },
    DrainingInput {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    TerminationRequested {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    },
    Terminated {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        reason: sdk::RunTerminationReason,
    },
    CancellationRequested {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    AwaitingUser {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        request_id: sdk::InteractionRequestId,
    },
    Resumed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        request_id: sdk::InteractionRequestId,
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
        /// 用户取消了本 Run 的某个 Step；Run 仅因完成 drain/seal 而进入 Completed。
        /// 消费方必须据此投影用户可见的取消终态，而不是正常完成。
        user_cancelled_step: bool,
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
            Self::Transitioned { parent_run_id, .. }
            | Self::Started { parent_run_id, .. }
            | Self::StepStarted { parent_run_id, .. }
            | Self::StepCompleted { parent_run_id, .. }
            | Self::StepCancellationRequested { parent_run_id, .. }
            | Self::StepFinalizationStarted { parent_run_id, .. }
            | Self::StepCancelled { parent_run_id, .. }
            | Self::DrainingInput { parent_run_id, .. }
            | Self::TerminationRequested { parent_run_id, .. }
            | Self::Terminated { parent_run_id, .. }
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
