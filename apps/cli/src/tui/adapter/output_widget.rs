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
    // 暂时启用 auto_scroll，防止 push_line 在全量重建时逐行递增 scroll_offset
    let saved_auto_scroll = output_area.auto_scroll;
    output_area.auto_scroll = true;
    for line in lines {
        output_area.push_line(OutputLine {
            content: line_to_plain_text(&line),
            style: LineStyle::Normal,
            ..Default::default()
        });
    }
    output_area.auto_scroll = saved_auto_scroll;
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

    /// 回归测试：全量替换时 push_line 不应逐行递增 scroll_offset
    ///
    /// 场景：用户滚动了 5 行后 streaming 新内容到达，ViewModel 全量替换 lines。
    /// 旧代码：lines.clear() 后 push_line 每行 scroll_offset+=1，导致从 5 累加到 N+5，
    /// clamp 后变成 max_offset 而非 0，auto_scroll 永不为 true。
    #[test]
    fn test_replace_does_not_accumulate_scroll_offset_per_line() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 20;
        output_area.auto_scroll = false;
        output_area.scroll_offset = 5;

        // 替换为 100 行内容
        let lines: Vec<Line<'static>> =
            (0..100).map(|i| Line::raw(format!("line {}", i))).collect();
        replace_lines_from_view_model(&mut output_area, lines);

        // scroll_offset 应保持 5（相对于可见区域顶端），而非被 push_line 累加到 105
        assert_eq!(output_area.scroll_offset, 5);
        assert!(!output_area.auto_scroll);
    }

    /// 全量替换时 auto_scroll=true 的情况不受影响
    #[test]
    fn test_replace_with_auto_scroll_stays_at_bottom() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 20;
        output_area.auto_scroll = true;
        output_area.scroll_offset = 0;

        let lines: Vec<Line<'static>> =
            (0..100).map(|i| Line::raw(format!("line {}", i))).collect();
        replace_lines_from_view_model(&mut output_area, lines);

        assert_eq!(output_area.scroll_offset, 0);
        assert!(output_area.auto_scroll);
    }
}
