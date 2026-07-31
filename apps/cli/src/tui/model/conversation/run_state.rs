use super::interaction::UiRunId;
use crate::tui::adapter::tui_runtime_event::TuiRunStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunStateSnapshot {
    pub(crate) run_id: UiRunId,
    pub(crate) parent_run_id: Option<UiRunId>,
    pub(crate) status: TuiRunStatus,
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
