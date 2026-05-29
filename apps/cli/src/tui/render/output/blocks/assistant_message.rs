use crate::tui::render::output::markdown::{is_table_row, is_table_separator};
use crate::tui::render::output::primitives::{
    markdown::markdown, rendered_line_from_spanparts, table::table,
};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::{syntax, theme};
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_assistant_message(
    block_id: &str,
    view: &TextBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let base = Style::default().fg(theme::ASSISTANT);
    let mut lines: Vec<RenderedLine> = Vec::new();
    let src = view.text.lines().collect::<Vec<_>>();
    let mut idx = 0;
    let mut in_fence = false;
    let mut fence_lang: Option<String> = None;

    while idx < src.len() {
        let line = src[idx];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
                fence_lang = None;
            } else {
                in_fence = true;
                fence_lang = Some(trimmed.trim_start_matches('`').trim().to_string());
            }
            lines.push(RenderedLine::new(vec![Span::styled(
                line.to_string(),
                Style::default().fg(theme::TEXT_DIM),
            )]));
            idx += 1;
            continue;
        }

        if in_fence {
            let syntax_ref = fence_lang
                .as_deref()
                .and_then(syntax::language_by_extension);
            if let Some(parts) = syntax::highlight_line(line, syntax_ref.as_ref()) {
                lines.push(rendered_line_from_spanparts(&parts));
            } else {
                lines.push(RenderedLine::new(vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(theme::CODE),
                )]));
            }
            idx += 1;
            continue;
        }

        if is_table_row(line) && idx + 1 < src.len() && is_table_separator(src[idx + 1]) {
            let mut end = idx;
            while end < src.len() && is_table_row(src[end]) {
                end += 1;
            }
            let block_src: Vec<&str> = src.iter().skip(idx).take(end - idx).copied().collect();
            lines.extend(table(&block_src, base, ctx.width));
            idx = end;
            continue;
        }

        lines.extend(markdown(line, base, ctx.width));
        idx += 1;
    }

    if lines.is_empty() {
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
    use crate::tui::view_model::style::SemanticStyle;
    use ratatui::style::Modifier;

    fn render(text: &str) -> RenderedBlock {
        let view = TextBlockView {
            key: "a".into(),
            text: text.into(),
            style: SemanticStyle::Normal,
        };
        render_assistant_message("a", &view, &RenderCtx { width: 80 })
    }

    #[test]
    fn test_assistant_renders_markdown_bold() {
        let block = render("see **this**");

        assert!(block.lines.iter().any(|line| line
            .spans
            .iter()
            .any(|span| span.content.as_ref() == "this"
                && span.style.add_modifier.contains(Modifier::BOLD))));
        assert!(block
            .lines
            .iter()
            .any(|line| line.plain.contains("see this")));
    }

    #[test]
    fn test_assistant_cjk_text_does_not_wrap_per_character_at_normal_width() {
        let block = render("整理一轮，不改代码。");

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, "整理一轮，不改代码。");
    }

    #[test]
    fn test_assistant_base_color_is_assistant_theme() {
        let block = render("plain text");

        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::ASSISTANT));
    }

    #[test]
    fn test_assistant_fence_does_not_leak_style_after_close() {
        let block = render("```\ncode\n```\nafter");
        let after = block.lines.last().unwrap();

        assert_eq!(after.plain, "after");
        assert_ne!(after.spans[0].style.fg, Some(theme::CODE));
    }
}
