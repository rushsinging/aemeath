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
        running: bool,
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
        message: String,
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
    /// Agent 工具的 role/model 元数据已写入（issue #499）。
    AgentMetaUpdated {
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
    AgentRunChanged {
        run_id: super::interaction::UiRunId,
        phase: super::interaction::AgentRunPhase,
    },
    AgentRunStepChanged {
        run_id: super::interaction::UiRunId,
        step_id: super::interaction::UiRunStepId,
        phase: super::interaction::AgentRunStepPhase,
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
    /// Compact 进度条嵌入 spinner 行（output 区），与 phase 变化解耦——单独归类为 output_dirty，
    /// 避免依赖 SpinnerTick 每 90ms 兜底 mark_output_dirty 的不可靠时序（#540）。
    CompactProgressChanged,
    SpinnerPhaseChanged,
    SpinnerStopped,
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
