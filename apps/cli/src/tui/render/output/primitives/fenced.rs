//! fenced markdown block orchestration.

use crate::tui::render::output::primitives::{
    blocks::{parse_blocks, MarkdownBlock},
    markdown::markdown,
    table::table,
    unified_diff::render_unified_diff,
    wrap::wrap_spans_to_rendered_lines,
};
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output::spacing::{MarkdownElement, MarkdownSpacingPolicy};
use crate::tui::render::{syntax, theme};
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_fenced_markdown(
    text: &str,
    base_style: Style,
    width: u16,
    policy: &MarkdownSpacingPolicy,
) -> Vec<RenderedLine> {
    let blocks = parse_blocks(text);
    let mut output = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        let gap = if index == 0 {
            policy.leading_gap(block.kind)
        } else {
            policy.boundary_gap(blocks[index - 1].kind, block.kind, block.source_gap_before)
        };
        output.extend(std::iter::repeat_with(RenderedLine::default).take(gap as usize));
        output.extend(render_block(block, base_style, width));
    }

    if let Some(last) = blocks.last() {
        output.extend(
            std::iter::repeat_with(RenderedLine::default)
                .take(policy.trailing_gap(last.kind) as usize),
        );
    }
    output
}

fn render_block(block: &MarkdownBlock<'_>, base_style: Style, width: u16) -> Vec<RenderedLine> {
    match block.kind {
        MarkdownElement::Table
            if width >= crate::tui::render::output::gutter::NARROW_DISABLE_TABLE_THRESHOLD =>
        {
            table(&block.lines, base_style, width)
        }
        MarkdownElement::CodeBlock => render_code_block(block, base_style, width),
        _ => markdown(&block.lines.join("\n"), base_style, width),
    }
}

fn render_code_block(
    block: &MarkdownBlock<'_>,
    base_style: Style,
    width: u16,
) -> Vec<RenderedLine> {
    let language = block.fence_language.as_deref();
    let hide_markers = language == Some("text");
    let has_closing = block.lines.len() > 1
        && block
            .lines
            .last()
            .is_some_and(|line| line.trim_start().starts_with("```"));
    let content_end = block.lines.len().saturating_sub(usize::from(has_closing));
    let content = &block.lines[1.min(block.lines.len())..content_end];
    let mut output = Vec::new();

    if !hide_markers {
        if let Some(opening) = block.lines.first() {
            output.push(RenderedLine::new(vec![Span::styled(
                (*opening).to_string(),
                Style::default().fg(theme::TEXT_DIM),
            )]));
        }
    }

    for line in content {
        if hide_markers {
            output.extend(markdown(line, base_style, width));
        } else if language == Some("diff") {
            output.extend(render_unified_diff(line, None, width));
        } else {
            let syntax_ref = language.and_then(syntax::language_by_fence_info);
            if let Some(parts) = syntax::highlight_line(line, syntax_ref.as_ref()) {
                output.extend(wrap_spans_to_rendered_lines(
                    crate::tui::render::output::primitives::spanparts_to_spans(&parts),
                    width as usize,
                ));
            } else {
                output.extend(wrap_spans_to_rendered_lines(
                    vec![Span::styled(
                        (*line).to_string(),
                        Style::default().fg(theme::CODE),
                    )],
                    width as usize,
                ));
            }
        }
    }

    if !hide_markers && has_closing {
        output.push(RenderedLine::new(vec![Span::styled(
            block.lines.last().unwrap().to_string(),
            Style::default().fg(theme::TEXT_DIM),
        )]));
    }
    output
}

#[cfg(test)]
#[path = "fenced_tests.rs"]
mod tests;
