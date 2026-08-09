use super::interaction::{InteractionCommandFailure, UiInteractionReply, UiInteractionRequestId};
use super::tool_call::ToolCallStatus;

#[derive(Clone, Debug, PartialEq)]
pub enum ConversationChange {
    // ── 原 conversation changes ──
    ChatStarted {
        chat_id: String,
    },
    ChatTurnStarted {
        chat_id: String,
        run_id: String,
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
        chat_id: String,
        run_id: String,
        id: String,
        name: String,
        running: bool,
    },
    ToolCallCompleted {
        chat_id: String,
        run_id: String,
        id: String,
        status: ToolCallStatus,
    },
    SystemMessageAppended {
        block_id: String,
    },
    ErrorAppended {
        block_id: String,
        message: String,
    },
    QueuedSubmissionAdded {
        id: String,
    },
    QueuedSubmissionsCleared {
        count: usize,
    },
    AgentActivitiesRecorded {
        block_id: String,
        tool_id: String,
    },
    /// 工具 stdout 流式输出已写入 ToolCall.activities，需 invalidate 该 block 的 root_cache。
    ToolStreamingOutputRecorded {
        block_id: String,
    },
    /// Agent 工具的 role/model 元数据已写入（issue #499）。
    AgentMetaUpdated {
        chat_id: String,
        run_id: String,
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
    AskUserDismissed {
        id: String,
    },
    InteractionShown {
        request_id: UiInteractionRequestId,
    },
    InteractionUpdated {
        request_id: UiInteractionRequestId,
    },
    InteractionReplyRequested {
        request_id: UiInteractionRequestId,
        reply: UiInteractionReply,
    },
    InteractionCancelRequested {
        request_id: UiInteractionRequestId,
    },
    InteractionCompleted {
        request_id: UiInteractionRequestId,
    },
    InteractionCommandRejected {
        request_id: UiInteractionRequestId,
        failure: InteractionCommandFailure,
    },
    InteractionConflict {
        active_request_id: UiInteractionRequestId,
        received_request_id: UiInteractionRequestId,
    },
    ActivityObservationChanged {
        run_id: super::interaction::UiRunId,
        activity_id: crate::tui::adapter::tui_runtime_event::UiActivityId,
    },
    ActivityObservationStale {
        run_id: super::interaction::UiRunId,
    },
    ActivitySnapshotReplaced {
        run_id: super::interaction::UiRunId,
    },
    OutputDirty,
    StyleBoundaryResetRequired,
    // ── 原 runtime changes（RuntimeChange 合入）──
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
    QueuedSubmissionsSynced {
        count: usize,
    },
    CompactRuntimeCleared,
    TaskLinesChanged,
    StatusNoticeChanged,
    GraphPhaseChanged,
}

impl ConversationChange {
    pub(crate) fn is_interaction_conflict(&self) -> bool {
        matches!(self, Self::InteractionConflict { .. })
    }

    pub(crate) fn is_interaction_reply_requested(&self) -> bool {
        matches!(self, Self::InteractionReplyRequested { .. })
    }
}
