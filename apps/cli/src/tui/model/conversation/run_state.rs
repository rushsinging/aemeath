use super::interaction::UiRunId;
use crate::tui::adapter::tui_runtime_event::TuiRunStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunStateSnapshot {
    pub(crate) run_id: UiRunId,
    pub(crate) parent_run_id: Option<UiRunId>,
    pub(crate) status: TuiRunStatus,
    pub(crate) timing_observation_revision: u64,
    pub(crate) total_elapsed_ms: u64,
    pub(crate) phase_elapsed_ms: u64,
}

impl RunStateSnapshot {
    pub(crate) fn is_main(&self) -> bool {
        self.parent_run_id.is_none()
    }
}

pub(crate) fn is_terminal(status: TuiRunStatus) -> bool {
    matches!(
        status,
        TuiRunStatus::Completed
            | TuiRunStatus::Failed
            | TuiRunStatus::Cancelled
            | TuiRunStatus::Terminated
    )
}
