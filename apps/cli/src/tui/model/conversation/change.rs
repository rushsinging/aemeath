use super::tool_call::ToolCallStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationChange {
    ChatStarted { chat_id: String },
    ChatTurnStarted { chat_id: String, turn_id: String },
    ToolCallObserved { name: String, index: usize },
    ToolCallBound { id: String, name: String },
    ToolCallCompleted { id: String, status: ToolCallStatus },
    ChatCompleting { chat_id: String },
    ChatCompleted { chat_id: String },
    OrphanToolResultObserved { id: String },
}
