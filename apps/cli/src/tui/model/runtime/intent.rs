use super::spinner::SpinnerPhase;
use super::workspace::WorktreeKind;

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeIntent {
    SetProviderModel {
        provider: Option<String>,
        model_id: Option<String>,
    },
    UpdateWorkspace {
        cwd: String,
        worktree: Option<String>,
    },
    WorkspaceSnapshotReceived {
        path_base: Option<String>,
        working_root: Option<String>,
        branch: Option<String>,
        kind: WorktreeKind,
    },
    RecordUsage {
        input_tokens: u64,
        output_tokens: u64,
        last_input_tokens: u64,
        cost_usd: f64,
    },
    SetContextSize(u64),
    RecordLiveTps {
        tps: f64,
    },
    UpdateTaskStatus {
        total: usize,
        completed: usize,
        in_progress: usize,
    },
    StartProcessingJob {
        id: String,
        chat_id: Option<String>,
    },
    FinishProcessingJob {
        id: String,
        success: bool,
    },
    SetSpinnerPhase(SpinnerPhase),
    StopSpinner,
    UpdateTaskLines(Vec<String>),
}
