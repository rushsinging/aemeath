use super::style::SemanticStyle;
use crate::tui::model::conversation::block::HookNoticeKind;
use std::hash::Hash;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputViewModel {
    pub roots: Vec<BlockNode>,
    pub version: u64,
    pub follow_tail_hint: bool,
}

impl Default for OutputViewModel {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            version: 0,
            follow_tail_hint: true,
        }
    }
}

/// 渲染树节点。children 为子块（如 tool result 子块）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockNode {
    pub block_id: String,
    pub block_version: u64,
    pub kind: OutputBlockKind,
    pub children: Vec<BlockNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum OutputBlockKind {
    UserMessage(TextBlockView),
    AssistantMessage(TextBlockView),
    ThinkingMessage(TextBlockView),
    ToolCall(ToolCallBlockView),
    ToolResult(ToolResultBlockView),
    HookNotice(HookNoticeBlockView),
    DiagnosticNotice(TextBlockView),
    SystemNotice(TextBlockView),
    AskUser(AskUserBlockView),
    /// 预留：分隔块（后续渲染管线 S 任务接线，当前仅测试构造）。
    #[allow(dead_code)]
    Separator,
}

/// AskUserQuestion 交互块视图：问题 + 选项列表 + 当前导航高亮状态。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserBlockView {
    pub key: String,
    pub question: String,
    pub options: Vec<sdk::OptionItem>,
    /// LLM 选项数量（内建项从该索引开始，不显示勾选框）。
    pub llm_option_count: usize,
    pub multi_select: bool,
    /// 当前光标所在选项索引（高亮行）。
    pub cursor: usize,
    /// multi_select 下各选项勾选状态。
    pub selected: Vec<bool>,
    /// 处于自由输入态（光标在 Type something 位置）。
    pub chat_input_active: bool,
    /// Type something 输入框中的文本。
    pub chat_input_text: String,
    /// 无选项自由输入模式下的默认值提示（渲染 `(default: ...)` 行）。
    pub default: Option<String>,
    /// 用户回答内容。Some 表示已回答，渲染为问答对。
    pub answer: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextBlockView {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct HookNoticeBlockView {
    pub key: String,
    pub kind: HookNoticeKind,
    pub title: String,
    pub body: String,
    pub details: Option<String>,
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

/// 工具结果子块视图：作为 ToolCall 的子节点，独占结果富渲染。
///
/// - `summary`：工具入参 JSON（用于 Edit diff 语法高亮扩展名推断），同 `ToolCallBlockView.summary`。
/// - `result_text`：结果摘要文本（源同 assembler 的 `tool_result_summary`）。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ToolResultBlockView {
    pub key: String,
    pub tool_title: String,
    pub summary: Option<String>,
    pub result_text: String,
    pub style: SemanticStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ToolSemanticStatus {
    /// 预留：工具尚未开始执行（当前 assembler 未产出此状态）。
    #[allow(dead_code)]
    Pending,
    Running,
    Success,
    Error,
    Cancelled,
    Orphaned,
}

#[cfg(test)]
mod node_tests {
    use super::*;

    fn leaf(id: &str) -> BlockNode {
        let kind = OutputBlockKind::Separator;
        BlockNode {
            block_id: id.into(),
            block_version: 0,
            kind,
            children: Vec::new(),
        }
    }

    #[test]
    fn test_block_node_leaf_has_no_children() {
        let n = leaf("a");
        assert!(n.children.is_empty());
    }

    #[test]
    fn test_block_node_can_nest_child() {
        let mut parent = leaf("p");
        parent.children.push(leaf("c"));
        assert_eq!(parent.children[0].block_id, "c");
    }

    #[test]
    fn test_output_view_model_roots_default_empty() {
        let vm = OutputViewModel::default();
        assert!(vm.roots.is_empty());
    }
}
