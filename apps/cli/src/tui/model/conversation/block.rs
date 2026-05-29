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
        name: String,
        summary: String,
        args_preview: String,
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
    /// AskUserQuestion 交互块（问题 + 选项列表）。渲染与选项导航高亮的单一真相。
    ///
    /// 选项导航的可变状态（`cursor`、`selected`、`chat_input_active`）随键盘交互
    /// 派发 intent 写入本块，渲染组件据此高亮，避免命令式重写输出行。
    AskUser {
        id: String,
        question: String,
        /// 全部选项（LLM 选项 + 内建选项）。
        options: Vec<String>,
        /// LLM 提供的选项数量（内建选项从该索引开始，不可在 multi_select 下勾选）。
        llm_option_count: usize,
        multi_select: bool,
        /// 当前光标所在选项索引（导航高亮的单一真相）。
        cursor: usize,
        /// multi_select 下各选项是否已勾选。
        selected: Vec<bool>,
        /// 是否处于「Chat about this...」自由输入子态（此时不高亮选项）。
        chat_input_active: bool,
        /// 无选项自由输入模式下的默认值提示。
        default: Option<String>,
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
            | ConversationBlock::OrphanToolResult { id, .. }
            | ConversationBlock::AskUser { id, .. } => id,
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
            text: "hello".to_string(),
        };
        assert_eq!(block.id(), "assistant-1");
    }

    #[test]
    fn test_conversation_block_returns_tool_id() {
        let block = ConversationBlock::ToolCall {
            id: ToolCallId::new("tool-1"),
            name: "Read".to_string(),
            summary: "read file".to_string(),
            args_preview: String::new(),
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
