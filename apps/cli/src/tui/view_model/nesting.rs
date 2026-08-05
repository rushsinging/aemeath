//! ViewModel block nesting legality rules.
use crate::tui::view_model::output::OutputBlockKind;

/// 最大嵌套深度：top(0) → tool_group(1) → tool_call(2) → result-content(3)。深度从 0 计，最深合法子层级为 3。
pub const MAX_BLOCK_DEPTH: usize = 4;

/// ToolGroup 仅可含 ToolCall；ToolCall 可含 ToolResult 结果子块，或既有文本/notice 子块；其余为叶子。
pub fn allowed_child(parent: &OutputBlockKind, child: &OutputBlockKind) -> bool {
    match parent {
        OutputBlockKind::ToolGroup(_) => matches!(child, OutputBlockKind::ToolCall(_)),
        OutputBlockKind::ToolCall(_) => matches!(
            child,
            OutputBlockKind::ToolResult(_)
                | OutputBlockKind::AssistantMessage(_)
                | OutputBlockKind::DiagnosticNotice(_)
                | OutputBlockKind::SystemNotice(_)
        ),
        _ => false,
    }
}

#[cfg(test)]
#[path = "nesting_tests.rs"]
mod tests;
