use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

pub fn semantic_color(style: SemanticStyle) -> Color {
    match style {
        SemanticStyle::Normal => theme::TEXT,
        SemanticStyle::Muted => theme::TEXT_MUTED,
        SemanticStyle::Running => theme::TOOL_RUNNING,
        SemanticStyle::Success => theme::SUCCESS,
        SemanticStyle::Error => theme::ERROR,
        SemanticStyle::Warning => theme::WARNING,
        SemanticStyle::Accent => theme::ACCENT,
    }
}

pub fn render_diagnostic(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(semantic_color(view.style));
    let mut lines: Vec<RenderedLine> = view
        .text
        .lines()
        .map(|line| RenderedLine::new(vec![Span::styled(line.to_string(), style)]))
        .collect();
    // 文本以换行结尾视为「显式尾随空行」（由块组件承担间距，如 done 提示与后续内容分隔）。
    if view.text.ends_with('\n') {
        lines.push(RenderedLine::default());
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::blocks::separator::render_separator;

    #[test]
    fn test_diagnostic_error_uses_error_color() {
        let view = TextBlockView {
            key: "e".into(),
            text: "boom".into(),
            style: SemanticStyle::Error,
        };
        let block = render_diagnostic("e", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "boom");
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::ERROR));
    }

    #[test]
    fn test_diagnostic_trailing_newline_emits_blank_line() {
        // 文本以 \n 结尾时追加一行尾随空行（done 提示间距，修迁移回归）。
        let view = TextBlockView {
            key: "d".into(),
            text: "✻ Sautéed for 3s\n".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_diagnostic("d", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines.len(), 2, "应有提示行 + 尾随空行");
        assert_eq!(block.lines[0].plain, "✻ Sautéed for 3s");
        assert_eq!(block.lines[1].plain, "", "末行为空行间距");
    }

    #[test]
    fn test_diagnostic_no_trailing_newline_no_extra_blank() {
        // 边界：不以 \n 结尾的普通提示不追加空行。
        let view = TextBlockView {
            key: "d".into(),
            text: "plain".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_diagnostic("d", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, "plain");
    }

    #[test]
    fn test_separator_emits_blank_line() {
        let block = render_separator("sep-0");

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, "");
    }
}
