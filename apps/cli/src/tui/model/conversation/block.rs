use super::ids::{ChatId, ChatRunId, ToolCallId};

/// AskUserQuestion 批量交互中的单个问题槽位。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserSlot {
    /// 对应的 tool_call_id。
    pub id: String,
    /// 同一 tool call 内的问题序号，从 0 开始。
    pub question_seq: usize,
    pub question: String,
    /// 全部选项（LLM 选项 + 内建选项）。
    pub options: Vec<sdk::OptionItem>,
    /// LLM 选项数量（内建选项从该索引开始）。
    pub llm_option_count: usize,
    pub multi_select: bool,
    pub default: Option<String>,
    /// 用户回答。None=未答，Some=已答。
    pub answer: Option<String>,
}

/// AskUser 批量交互的完成状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AskUserCompletion {
    Active,
    ReplyPending,
    CancelPending,
    Answered,
    Cancelled,
}

impl AskUserCompletion {
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Answered | Self::Cancelled)
    }
}

/// AskUser 批量交互的阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AskUserPhase {
    /// 逐个回答中。
    Answering,
    /// 全部答完，等待确认。
    Confirming,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationBlock {
    UserMessage {
        id: String,
        text: String,
    },
    AssistantText {
        id: String,
        chat_id: Option<ChatId>,
        run_id: Option<ChatRunId>,
        text: String,
    },
    Thinking {
        id: String,
        chat_id: Option<ChatId>,
        run_id: Option<ChatRunId>,
        text: String,
    },
    ToolCall {
        id: ToolCallId,
        chat_id: ChatId,
        run_id: ChatRunId,
    },
    ToolResult {
        id: ToolCallId,
        chat_id: ChatId,
        run_id: ChatRunId,
        output: String,
        content: serde_json::Value,
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
        input_id: String,
        text: String,
    },
    OrphanToolResult {
        id: String,
        /// 产生该结果的工具名（结果早于 ToolCall 绑定到达）。用于渲染工具摘要，
        /// 避免把完整原始 output 当正文刷出（#87 残留）。
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
    },
    /// AskUserQuestion 批量交互块（多问 + 确认页状态机）。
    AskUserBatch {
        id: String,
        /// 所有问题槽位。
        slots: Vec<AskUserSlot>,
        /// 当前激活的问题索引。
        active_index: usize,
        /// 交互阶段。
        phase: AskUserPhase,
        // ── 当前激活问题的选项导航状态 ──
        /// 当前激活问题的选项光标。
        cursor: usize,
        /// 当前激活问题的 multi_select 勾选状态。
        selected: Vec<bool>,
        /// 是否处于 Type something 自由输入子态。
        chat_input_active: bool,
        /// Type something 输入框文本。
        chat_input_text: String,
        /// Type something 输入框的光标位置（byte offset）。
        chat_input_cursor: usize,
        /// 确认页导航光标。
        confirm_cursor: usize,
        /// Runtime 确认的完成状态。
        completion: AskUserCompletion,
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
            | ConversationBlock::OrphanToolResult { id, .. }
            | ConversationBlock::AskUserBatch { id, .. } => id,
            ConversationBlock::ToolCall { id, .. } | ConversationBlock::ToolResult { id, .. } => {
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
            chat_id: None,
            run_id: None,
            text: "hello".to_string(),
        };
        assert_eq!(block.id(), "assistant-1");
    }

    #[test]
    fn test_conversation_block_returns_tool_id() {
        let block = ConversationBlock::ToolCall {
            id: ToolCallId::new("tool-1"),
            chat_id: ChatId::new("chat-1"),
            run_id: ChatRunId::new("turn-1"),
        };
        let _ = block.id(); // just verify it returns
    }

    #[test]
    fn test_conversation_block_distinguishes_orphan_result() {
        let block = ConversationBlock::OrphanToolResult {
            id: "missing".to_string(),
            tool_name: "Read".to_string(),
            output: "late".to_string(),
            content: serde_json::json!({ "text": "late" }),
            is_error: false,
        };
        assert!(matches!(block, ConversationBlock::OrphanToolResult { .. }));
    }
}
