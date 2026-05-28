use super::ids::ToolCallId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationBlock {
    UserMessage {
        id: String,
        text: String,
    },
    AssistantText {
        id: String,
        text: String,
    },
    Thinking {
        id: String,
        text: String,
    },
    ToolCall {
        id: ToolCallId,
    },
    ToolResult {
        id: ToolCallId,
        output: String,
        is_error: bool,
        image_count: usize,
    },
    System {
        id: String,
        text: String,
    },
    Error {
        id: String,
        text: String,
    },
    QueuedUserMessage {
        id: String,
        text: String,
    },
    AgentProgress {
        id: String,
        tool_id: String,
        message: String,
    },
    OrphanToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
}

impl ConversationBlock {
    pub fn id(&self) -> &str {
        match self {
            ConversationBlock::UserMessage { id, .. }
            | ConversationBlock::AssistantText { id, .. }
            | ConversationBlock::Thinking { id, .. }
            | ConversationBlock::System { id, .. }
            | ConversationBlock::Error { id, .. }
            | ConversationBlock::QueuedUserMessage { id, .. }
            | ConversationBlock::AgentProgress { id, .. }
            | ConversationBlock::OrphanToolResult { id, .. } => id,
            ConversationBlock::ToolCall { id } | ConversationBlock::ToolResult { id, .. } => {
                id.as_ref()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::ToolCallId;

    #[test]
    fn test_conversation_block_returns_text_id() {
        let block = ConversationBlock::AssistantText {
            id: "assistant-1".to_string(),
            text: "hello".to_string(),
        };
        assert_eq!(block.id(), "assistant-1");
    }

    #[test]
    fn test_conversation_block_returns_tool_id() {
        let block = ConversationBlock::ToolCall {
            id: ToolCallId::new("tool-1"),
        };
        assert_eq!(block.id(), "tool-1");
    }

    #[test]
    fn test_conversation_block_distinguishes_orphan_result() {
        let block = ConversationBlock::OrphanToolResult {
            id: "missing".to_string(),
            output: "late".to_string(),
            is_error: false,
        };
        assert!(matches!(block, ConversationBlock::OrphanToolResult { .. }));
    }
}
