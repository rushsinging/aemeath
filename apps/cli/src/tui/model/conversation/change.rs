use super::tool_call::ToolCallStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationChange {
    ChatStarted { chat_id: String },
    ChatTurnStarted { chat_id: String, turn_id: String },
    UserMessageAppended { block_id: String },
    AssistantTextAppended { block_id: String },
    ThinkingTextAppended { block_id: String },
    BlockCompleted { block_id: Option<String> },
    ToolCallObserved { name: String, index: usize },
    ToolCallBound { id: String, name: String },
    ToolCallCompleted { id: String, status: ToolCallStatus },
    SystemMessageAppended { block_id: String },
    ErrorAppended { block_id: String },
    QueuedSubmissionAdded { id: String },
    QueuedSubmissionsCleared { count: usize },
    AgentProgressRecorded { block_id: String, tool_id: String },
    ChatCompleting { chat_id: String },
    ChatCompleted { chat_id: String },
    OrphanToolResultObserved { id: String },
    AskUserShown { id: String },
    AskUserUpdated { id: String },
    AskUserDismissed,
    OutputDirty,
    StyleBoundaryResetRequired,
}
