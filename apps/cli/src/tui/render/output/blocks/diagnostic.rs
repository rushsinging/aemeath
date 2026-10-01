use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::rc::Rc;

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
    render_text_lines(block_id, &view.text, view.style)
}

fn render_text_lines(block_id: &str, text: &str, semantic_style: SemanticStyle) -> RenderedBlock {
    let style = Style::default().fg(semantic_color(semantic_style));
    let mut lines: Vec<RenderedLine> = text
        .lines()
        .map(|line| RenderedLine::new(vec![Span::styled(line.to_string(), style)]))
        .collect();
    // 文本以换行结尾视为「显式尾随空行」（由块组件承担间距，如 done 提示与后续内容分隔）。
    if text.ends_with('\n') {
        lines.push(RenderedLine::default());
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

#[cfg(test)]
#[path = "diagnostic_tests.rs"]
mod tests;
