use super::ids::{ChatId, ChatTurnId, ToolCallId};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HookNoticeKind {
    Blocked,
    Failed,
    Info,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct HookNoticeContent {
    pub kind: HookNoticeKind,
    pub title: String,
    pub body: String,
    pub details: Option<String>,
}

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
        turn_id: Option<ChatTurnId>,
        text: String,
    },
    Thinking {
        id: String,
        chat_id: Option<ChatId>,
        turn_id: Option<ChatTurnId>,
        text: String,
    },
    ToolCall {
        id: ToolCallId,
        chat_id: ChatId,
        turn_id: ChatTurnId,
    },
    ToolResult {
        id: ToolCallId,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    },
    System {
        id: String,
        text: String,
    },
    HookNotice {
        id: String,
        content: HookNoticeContent,
    },
    Error {
        id: String,
        text: String,
    },
    QueuedUserMessage {
        id: String,
        input_id: sdk::InputId,
        text: String,
    },
    AgentProgress {
        id: String,
        tool_id: ToolCallId,
        message: String,
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
        /// 用户已确认提交（block 进入终态）。
        confirmed: bool,
    },
}

impl ConversationBlock {
    pub fn id(&self) -> &str {
        match self {
            ConversationBlock::UserMessage { id, .. }
            | ConversationBlock::AssistantText { id, .. }
            | ConversationBlock::Thinking { id, .. }
            | ConversationBlock::System { id, .. }
            | ConversationBlock::HookNotice { id, .. }
            | ConversationBlock::Error { id, .. }
            | ConversationBlock::QueuedUserMessage { id, .. }
            | ConversationBlock::AgentProgress { id, .. }
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
            turn_id: None,
            text: "hello".to_string(),
        };
        assert_eq!(block.id(), "assistant-1");
    }

    #[test]
    fn test_conversation_block_returns_tool_id() {
        let block = ConversationBlock::ToolCall {
            id: ToolCallId::new("tool-1"),
            chat_id: ChatId::new("chat-1"),
            turn_id: ChatTurnId::new("turn-1"),
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
