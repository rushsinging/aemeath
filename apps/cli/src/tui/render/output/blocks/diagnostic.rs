use crate::tui::render::output::primitives::wrap::wrap_styled_text;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;
use ratatui::style::{Color, Style};
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

pub fn render_diagnostic(block_id: &str, view: &TextBlockView, ctx: &RenderCtx) -> RenderedBlock {
    render_text_lines(
        block_id,
        &view.text,
        view.style,
        usize::from(ctx.text_width),
    )
}

/// 系统提示 / 诊断提示的渲染。
///
/// 走统一的 width-aware wrap 管线（`wrap_styled_text`），不再自建「一行一条」输出：
/// 升级提示、错误提示常含长 URL 或长句子，忽略宽度会横向溢出且窗口变化后不重排。
fn render_text_lines(
    block_id: &str,
    text: &str,
    semantic_style: SemanticStyle,
    max_width: usize,
) -> RenderedBlock {
    let style = Style::default().fg(semantic_color(semantic_style));
    let mut lines = wrap_styled_text(text, style, max_width);
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
