//! 行首标志槽 gutter：depth 缩进 + marker 列。组合期注入，只进 spans 不进 plain。
//! marker 静态（按 kind/status），仅首行画，后续行等宽空白。见 spec §6.5。

use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use crate::tui::view_model::output::{OutputBlockKind, ToolSemanticStatus};
use ratatui::style::Style;
use ratatui::text::Span;

/// marker 列字符宽度（字形 1 + 空格 1）。
pub const GUTTER_WIDTH: usize = 2;
const PER_DEPTH_INDENT: usize = 2;

/// 按 block 类型 / 工具状态映射 marker 字形。字形均为单列显示宽度。
pub fn marker_glyph(kind: &OutputBlockKind) -> &'static str {
    match kind {
        OutputBlockKind::ToolCall(t) => match t.semantic_status {
            ToolSemanticStatus::Success => "✓",
            ToolSemanticStatus::Error => "✗",
            ToolSemanticStatus::Cancelled => "–",
            ToolSemanticStatus::Orphaned => "?",
            ToolSemanticStatus::Pending | ToolSemanticStatus::Running => "●",
        },
        OutputBlockKind::UserMessage(_) => ">",
        _ => " ",
    }
}

/// marker 字形的前景色（按 block 类型 / 工具状态）。
fn marker_color(kind: &OutputBlockKind) -> ratatui::style::Color {
    match kind {
        OutputBlockKind::ToolCall(t) => match t.semantic_status {
            ToolSemanticStatus::Success => theme::SUCCESS,
            ToolSemanticStatus::Error => theme::ERROR,
            ToolSemanticStatus::Running | ToolSemanticStatus::Pending => theme::TOOL_RUNNING,
            ToolSemanticStatus::Cancelled => theme::TEXT_MUTED,
            ToolSemanticStatus::Orphaned => theme::WARNING,
        },
        OutputBlockKind::UserMessage(_) => theme::USER,
        _ => theme::TEXT_MUTED,
    }
}

/// gutter 总显示宽度（供选区列偏移补偿用）。
pub fn gutter_width(depth: usize) -> usize {
    depth * PER_DEPTH_INDENT + GUTTER_WIDTH
}

/// 为一个 block 的所有行前置 gutter（首行带 marker，余行等宽空白）。gutter 只进 spans，不进 plain。
pub fn apply_gutter(
    kind: &OutputBlockKind,
    depth: usize,
    lines: Vec<RenderedLine>,
) -> Vec<RenderedLine> {
    let glyph = marker_glyph(kind);
    let color = marker_color(kind);
    let indent_n = depth * PER_DEPTH_INDENT;
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let gutter_text = if i == 0 {
                format!("{}{glyph} ", " ".repeat(indent_n))
            } else {
                " ".repeat(indent_n + GUTTER_WIDTH)
            };
            let mut spans = vec![Span::styled(gutter_text, Style::default().fg(color))];
            spans.extend(line.spans);
            let mut gutted = RenderedLine::with_plain(spans, line.plain);
            // gutter 文本均为宽度 1 字符（缩进空格 + 字形 + 空格），
            // 故 gutter_cols 同时等于前导显示列数与首 span 字符数。
            gutted.gutter_cols = gutter_width(depth);
            gutted
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::RenderedLine;
    use crate::tui::view_model::output::{
        OutputBlockKind, TextBlockView, ToolCallBlockView, ToolSemanticStatus,
    };
    use crate::tui::view_model::style::SemanticStyle;
    use ratatui::text::Span;

    fn tool(status: ToolSemanticStatus) -> OutputBlockKind {
        OutputBlockKind::ToolCall(ToolCallBlockView {
            key: "t".into(),
            chat_id: None,
            turn_id: None,
            tool_call_id: None,
            title: "Grep".into(),
            icon: "●".into(),
            semantic_status: status,
            style: SemanticStyle::Running,
            args_preview: None,
            summary: None,
            activity_summary: None,
            result_summary: None,
            collapsible: false,
            collapsed: false,
        })
    }

    #[test]
    fn test_marker_glyph_for_tool_status() {
        assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Success)), "✓");
        assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Error)), "✗");
        assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Running)), "●");
    }

    #[test]
    fn test_apply_gutter_first_line_has_marker_rest_blank_not_in_plain() {
        let kind = tool(ToolSemanticStatus::Success);
        let lines = vec![
            RenderedLine::new(vec![Span::raw("Grep /x/")]),
            RenderedLine::new(vec![Span::raw("detail")]),
        ];
        let out = apply_gutter(&kind, 0, lines);
        assert!(out[0].spans[0].content.as_ref().contains('✓'));
        assert_eq!(out[0].plain, "Grep /x/");
        assert!(out[1].spans[0].content.as_ref().chars().all(|c| c == ' '));
        assert_eq!(out[1].plain, "detail");
    }

    #[test]
    fn test_apply_gutter_depth_widens_indent() {
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: "s".into(),
            text: "x".into(),
            style: SemanticStyle::Muted,
        });
        let d0 = apply_gutter(&kind, 0, vec![RenderedLine::new(vec![Span::raw("x")])]);
        let d1 = apply_gutter(&kind, 1, vec![RenderedLine::new(vec![Span::raw("x")])]);
        let w0 = d0[0].spans[0].content.chars().count();
        let w1 = d1[0].spans[0].content.chars().count();
        assert!(w1 > w0, "depth 越深，gutter 前导越宽");
        assert_eq!(d1[0].plain, "x", "缩进不进 plain");
    }

    #[test]
    fn test_apply_gutter_sets_gutter_cols() {
        let kind = tool(ToolSemanticStatus::Success);
        let lines = vec![
            RenderedLine::new(vec![Span::raw("Grep")]),
            RenderedLine::new(vec![Span::raw("detail")]),
        ];
        let d0 = apply_gutter(&kind, 0, lines.clone());
        assert_eq!(d0[0].gutter_cols, gutter_width(0));
        assert_eq!(
            d0[1].gutter_cols,
            gutter_width(0),
            "续行 gutter_cols 同首行"
        );
        // gutter_cols 须等于首 span 字符数（不变式：均宽度 1 字符）。
        assert_eq!(d0[0].spans[0].content.chars().count(), d0[0].gutter_cols);

        let d1 = apply_gutter(&kind, 1, lines);
        assert_eq!(d1[0].gutter_cols, gutter_width(1));
        assert_eq!(d1[0].spans[0].content.chars().count(), d1[0].gutter_cols);
    }
}
