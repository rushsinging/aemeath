use super::style::SemanticStyle;
use std::hash::Hash;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputViewModel {
    pub blocks: Vec<OutputBlockView>,
    pub version: u64,
    pub follow_tail_hint: bool,
}

impl Default for OutputViewModel {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            version: 0,
            follow_tail_hint: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputBlockView {
    pub block_id: String,
    pub block_version: u64,
    pub kind: OutputBlockKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum OutputBlockKind {
    UserMessage(TextBlockView),
    QueuedSubmission(TextBlockView),
    AssistantMessage(TextBlockView),
    ThinkingMessage(TextBlockView),
    ToolCall(ToolCallBlockView),
    DiagnosticNotice(TextBlockView),
    SystemNotice(TextBlockView),
    AskUser(AskUserBlockView),
    Separator,
}

/// AskUserQuestion 交互块视图：问题 + 选项列表 + 当前导航高亮状态。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserBlockView {
    pub key: String,
    pub question: String,
    pub options: Vec<String>,
    /// LLM 选项数量（内建项从该索引开始，不显示勾选框）。
    pub llm_option_count: usize,
    pub multi_select: bool,
    /// 当前光标所在选项索引（高亮行）。
    pub cursor: usize,
    /// multi_select 下各选项勾选状态。
    pub selected: Vec<bool>,
    /// 处于「Chat about this...」自由输入子态时不高亮选项。
    pub chat_input_active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextBlockView {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ToolCallBlockView {
    pub key: String,
    pub chat_id: Option<String>,
    pub turn_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub title: String,
    pub icon: String,
    pub semantic_status: ToolSemanticStatus,
    pub style: SemanticStyle,
    pub args_preview: Option<String>,
    pub summary: Option<String>,
    pub activity_summary: Option<String>,
    pub result_summary: Option<String>,
    pub collapsible: bool,
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ToolSemanticStatus {
    Pending,
    Running,
    Success,
    Error,
    Cancelled,
    Orphaned,
}
