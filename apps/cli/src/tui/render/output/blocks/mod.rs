//! 顶层 block 级渲染组件。每个组件 fn(view, ctx) -> RenderedBlock。

use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};
use crate::tui::view_model::output::OutputBlockKind;

pub mod assistant_message;
pub mod diagnostic;
pub mod queued_submission;
pub mod separator;
pub mod thinking;
pub mod tool_call;
pub mod user_message;

pub fn render_block(kind: &OutputBlockKind, block_id: &str, ctx: &RenderCtx) -> RenderedBlock {
    match kind {
        OutputBlockKind::AssistantMessage(text) => {
            assistant_message::render_assistant_message(block_id, text, ctx)
        }
        OutputBlockKind::ToolCall(tool) => tool_call::render_tool_call(block_id, tool, ctx),
        OutputBlockKind::ThinkingMessage(text) => thinking::render_thinking(block_id, text, ctx),
        OutputBlockKind::QueuedSubmission(text) => {
            queued_submission::render_queued_submission(block_id, text, ctx)
        }
        OutputBlockKind::UserMessage(text) => {
            user_message::render_user_message(block_id, text, ctx)
        }
        OutputBlockKind::Separator => separator::render_separator(block_id),
        OutputBlockKind::SystemNotice(text) | OutputBlockKind::DiagnosticNotice(text) => {
            diagnostic::render_diagnostic(block_id, text, ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::TextBlockView;
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_render_block_system_notice_routes_to_component() {
        let block = render_block(
            &OutputBlockKind::SystemNotice(TextBlockView {
                key: "s".into(),
                text: "ok".into(),
                style: SemanticStyle::Muted,
            }),
            "s",
            &RenderCtx { width: 80 },
        );

        assert_eq!(block.block_id, "s");
        assert_eq!(block.lines[0].plain, "ok");
    }
}
