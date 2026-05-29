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
    Separator,
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
