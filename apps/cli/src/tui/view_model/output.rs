use super::conversation::tool_result_payload::ToolResultPayload;
use super::style::SemanticStyle;
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
    AskUserBatch(AskUserBatchBlockView),
}

/// AskUserQuestion 批量交互块视图。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserBatchBlockView {
    pub key: String,
    pub slots: Vec<AskUserSlotView>,
    pub active_index: usize,
    pub phase: AskUserPhaseView,
    pub cursor: usize,
    pub selected: Vec<bool>,
    pub chat_input_active: bool,
    pub chat_input_text: String,
    pub chat_input_cursor: usize,
    pub confirm_cursor: usize,
    pub confirmed: bool,
}

/// AskUserBatch 单问槽位视图（投影自 model 层，不依赖 model internals）。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserSlotView {
    pub id: String,
    pub question: String,
    pub options: Vec<sdk::OptionItem>,
    pub llm_option_count: usize,
    pub multi_select: bool,
    pub default: Option<String>,
    pub answer: Option<String>,
}

/// AskUserBatch 阶段视图。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AskUserPhaseView {
    Answering,
    Confirming,
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextBlockView {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum HookNoticeSemanticKind {
    Blocked,
    Failed,
    Info,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct HookNoticeBlockView {
    pub key: String,
    pub kind: HookNoticeSemanticKind,
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
    pub activity_lines: Vec<String>,
    pub result_summary: Option<String>,
    /// Owned structured payload of the tool result (output/content/is_error/image_count).
    /// 用于 TUI Display 从 typed 字段渲染 header（line_count/bytes_written/diff 等），
    /// 不依赖手工解析 message 字符串。
    pub result_payload: Option<ToolResultPayload>,
    /// 当前 worktree 根（用于 tool header 路径相对化，issue #342）。
    /// 由 view_assembler 从 `WorkspaceService::current_workspace_root()` 填充。
    /// 纳入 `Hash` 派生 → worktree 切换时 `cache_version()` 自动变化，block 缓存自动失效。
    pub workspace_root: Option<std::path::PathBuf>,
    pub collapsible: bool,
    pub collapsed: bool,
    /// Agent 工具特化元数据（issue #499）。仅 `tool_name == "Agent"` 时填充。
    pub agent_meta: Option<AgentMetaView>,
}

/// Agent 工具元数据的 view 层投影（issue #499）。
///
/// 独立于 `model::conversation::tool_call::AgentMeta`，避免 view_model
/// 反向依赖 model internals（架构守卫：TUI view_model must not depend on
/// model internals）。view_assembler 负责从 model 层投影到此 view 类型。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AgentMetaView {
    /// sub-agent 的角色名（如 `reviewer`）。None 表示未指定 role。
    pub role: Option<String>,
    /// sub-agent 实际使用的 model（runtime resolve 后的值）。
    pub model: String,
}

/// 工具结果子块视图：作为 ToolCall 的子节点，独占结果富渲染。
///
/// - `args_preview`：工具入参 JSON（用于 Edit diff 语法高亮扩展名推断）。
/// - `summary`：人类可读摘要（如 `"L12-L34 (23 lines)"`）。
/// - `result_text`：结果摘要文本（源同 assembler 的 `tool_result_summary`）。
/// - `data`：结构化 typed result JSON（如 `EditResult`），供 diff 等富渲染直接读取（#546）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultBlockView {
    pub key: String,
    pub tool_title: String,
    pub args_preview: Option<String>,
    pub result_text: String,
    pub data: Option<serde_json::Value>,
    pub style: SemanticStyle,
}

// 手写 Hash：serde_json::Value 不 impl Hash，只 hash 标识字段（key/tool_title/result_text/
// args_preview/style），data 不参与缓存指纹（同 ToolResultPayload 的处理方式）。
impl std::hash::Hash for ToolResultBlockView {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.tool_title.hash(state);
        self.args_preview.hash(state);
        self.result_text.hash(state);
        self.style.hash(state);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ToolSemanticStatus {
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
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: id.into(),
            text: String::new(),
            style: SemanticStyle::Normal,
        });
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
