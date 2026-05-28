use crate::tui::render::output_area::{LineStyle, OutputArea, OutputLine};
use ratatui::text::Line;

pub(crate) fn replace_lines_from_view_model(
    output_area: &mut OutputArea,
    lines: Vec<Line<'static>>,
) {
    output_area.finish_streaming();
    output_area.lines.clear();
    output_area.rendered_line_content.clear();
    output_area.screen_line_map.clear();
    output_area.selection_start = None;
    output_area.selection_end = None;
    output_area.is_selecting = false;
    for line in lines {
        output_area.push_line(OutputLine {
            content: line_to_plain_text(&line),
            style: LineStyle::Normal,
            ..Default::default()
        });
    }
    clamp_scroll_state(output_area);
}

fn clamp_scroll_state(output_area: &mut OutputArea) {
    let max_offset = output_area
        .lines
        .len()
        .saturating_sub(output_area.last_visible_height);
    output_area.scroll_offset = output_area.scroll_offset.min(max_offset);
    if output_area.scroll_offset == 0 {
        output_area.auto_scroll = true;
    }
    output_area
        .rendered_cache
        .line_cache
        .content_changed(output_area.lines.len());
}

fn line_to_plain_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    #[test]
    fn test_line_to_plain_text_joins_spans() {
        let line = Line::from(vec![Span::raw("a"), Span::raw("b")]);

        assert_eq!(line_to_plain_text(&line), "ab");
    }

    #[test]
    fn test_replace_lines_from_view_model_replaces_existing_lines() {
        let mut output_area = OutputArea::new();
        output_area.push_system("old");

        replace_lines_from_view_model(&mut output_area, vec![Line::raw("new")]);

        assert_eq!(output_area.lines.len(), 1);
        assert_eq!(output_area.lines[0].content, "new");
    }

    #[test]
    fn test_replace_lines_from_view_model_handles_empty_lines() {
        let mut output_area = OutputArea::new();
        replace_lines_from_view_model(&mut output_area, vec![Line::raw("")]);

        assert_eq!(output_area.lines.len(), 1);
        assert!(output_area.lines[0].content.is_empty());
    }

    #[test]
    fn test_replace_lines_from_view_model_clamps_stale_scroll_offset() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 2;
        output_area.auto_scroll = false;
        output_area.scroll_offset = 100;

        replace_lines_from_view_model(&mut output_area, vec![Line::raw("only")]);

        assert_eq!(output_area.scroll_offset, 0);
        assert!(output_area.auto_scroll);
    }
}
