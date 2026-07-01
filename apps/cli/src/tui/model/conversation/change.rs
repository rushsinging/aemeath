use super::tool_call::ToolCallStatus;
use super::workspace::WorktreeKind;

#[derive(Clone, Debug, PartialEq)]
pub enum ConversationChange {
    // ── 原 conversation changes ──
    ChatStarted {
        chat_id: String,
    },
    ChatTurnStarted {
        chat_id: String,
        turn_id: String,
    },
    UserMessageAppended {
        block_id: String,
    },
    AssistantTextAppended {
        block_id: String,
    },
    ThinkingTextAppended {
        block_id: String,
    },
    BlockCompleted {
        block_id: Option<String>,
    },
    ToolCallObserved {
        name: String,
        index: usize,
    },
    ToolCallBound {
        id: String,
        name: String,
    },
    ToolCallCompleted {
        id: String,
        status: ToolCallStatus,
    },
    SystemMessageAppended {
        block_id: String,
    },
    ErrorAppended {
        block_id: String,
    },
    QueuedSubmissionAdded {
        id: String,
    },
    QueuedSubmissionsCleared {
        count: usize,
    },
    AgentProgressRecorded {
        block_id: String,
        tool_id: String,
    },
    ChatCompleting {
        chat_id: String,
    },
    ChatCompleted {
        chat_id: String,
    },
    OrphanToolResultObserved {
        id: String,
    },
    AskUserShown {
        id: String,
    },
    AskUserUpdated {
        id: String,
    },
    AskUserDismissed,
    OutputDirty,
    StyleBoundaryResetRequired,
    // ── 原 runtime changes（RuntimeChange 合入）──
    ProviderModelChanged {
        provider: Option<String>,
        model_id: Option<String>,
    },
    WorkspaceChanged {
        cwd: String,
        worktree: Option<String>,
    },
    WorkspaceSnapshotChanged {
        path_base: Option<String>,
        workspace_root: Option<String>,
        branch: Option<String>,
        kind: WorktreeKind,
    },
    UsageChanged {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    LiveTpsChanged {
        tps: f64,
    },
    TaskStatusChanged {
        total: usize,
        completed: usize,
        in_progress: usize,
    },
    ProcessingJobChanged {
        id: String,
    },
    SpinnerPhaseChanged,
    SpinnerStopped,
    TaskLinesChanged,
    StatusNoticeChanged,
    ThinkingChanged,
    GraphPhaseChanged,
}
