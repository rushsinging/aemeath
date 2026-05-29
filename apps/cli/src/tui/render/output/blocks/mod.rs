//! 顶层 block 级渲染组件。每个组件 fn(view, ctx) -> RenderedBlock。

pub mod ask_user;
pub mod assistant_message;
pub mod diagnostic;
pub mod edit_diff;
pub mod queued_submission;
pub mod separator;
pub mod thinking;
pub mod tool_call;
pub mod tool_result;
pub mod user_message;

#[cfg(test)]
mod tests {
    use crate::tui::render::output::block_component::BlockComponent;
    use crate::tui::render::output::rendered::RenderCtx;
    use crate::tui::view_model::output::{OutputBlockKind, TextBlockView};
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_render_block_assistant_after_system_does_not_inherit_dark() {
        // #74 回归：System 样式 block（Muted）后渲染 Assistant block 时，
        // 每个 block 独立从自身 kind/style 派生颜色，Assistant 不继承前一 block 的暗色。
        let ctx = RenderCtx { width: 80 };
        let _system = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "system muted".into(),
            style: SemanticStyle::Muted,
        })
        .component()
        .render_self("s", &ctx);
        let assistant = OutputBlockKind::AssistantMessage(TextBlockView {
            key: "a".into(),
            text: "assistant reply".into(),
            style: SemanticStyle::Normal,
        })
        .component()
        .render_self("a", &ctx);

        use crate::tui::render::theme;
        assert_eq!(
            assistant.lines[0].spans[0].style.fg,
            Some(theme::ASSISTANT),
            "Assistant block 应使用 ASSISTANT 前景色，不继承 System block 的暗色"
        );
    }

    #[test]
    fn test_render_block_system_notice_routes_to_component() {
        let block = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "ok".into(),
            style: SemanticStyle::Muted,
        })
        .component()
        .render_self("s", &RenderCtx { width: 80 });

        assert_eq!(block.block_id, "s");
        assert_eq!(block.lines[0].plain, "ok");
    }
}
