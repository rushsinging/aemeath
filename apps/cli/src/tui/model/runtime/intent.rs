use super::spinner::SpinnerPhase;
use super::status_notice::StatusNotice;
use super::workspace::WorktreeKind;
use std::time::Instant;

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
        workspace_root: Option<String>,
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
    /// 更新 last_input_tokens（compact 后重新估算上下文用）。
    UpdateLastInputTokens(u64),
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
    SetStatusNotice(StatusNotice),
    /// 临时 status notice，到 `expires_at` 后由 SpinnerTick 回退到 graph_phase 派生态。
    SetTransientStatusNotice {
        notice: StatusNotice,
        expires_at: Instant,
    },
    SetThinking(bool),
    /// Reasoning Graph 阶段变化。`None` 表示 graph 不存在或回 Idle。
    SetGraphPhase(Option<String>),
    /// Compact 进度更新。`None` 清除进度。
    SetCompactProgress {
        stage: String,
        current: Option<u32>,
        total: Option<u32>,
    },
}
