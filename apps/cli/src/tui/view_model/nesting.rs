//! ViewModel block nesting legality rules.
pub use super::constants::MAX_BLOCK_DEPTH;
use crate::tui::view_model::output::OutputBlockKind;

/// 仅 ToolCall 可含子（ToolResult 结果子块，或 AssistantMessage 文本 / Diagnostic / SystemNotice）；其余为叶子。
pub fn allowed_child(parent: &OutputBlockKind, child: &OutputBlockKind) -> bool {
    matches!(parent, OutputBlockKind::ToolCall(_))
        && matches!(
            child,
            OutputBlockKind::ToolResult(_)
                | OutputBlockKind::AssistantMessage(_)
                | OutputBlockKind::DiagnosticNotice(_)
                | OutputBlockKind::SystemNotice(_)
        )
}

#[cfg(test)]
#[path = "nesting_tests.rs"]
mod tests;
