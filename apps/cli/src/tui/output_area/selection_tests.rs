use super::super::{LineStyle, OutputArea, OutputLine};
use ::runtime::api::core::string_idx::CharIdx;
use ratatui::{buffer::Buffer, layout::Rect};

#[test]
fn test_get_selected_text_clamps_start_col_after_line_shrinks() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "短".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.selection_start = Some((0, CharIdx::new(4)));
    output.selection_end = Some((0, CharIdx::new(6)));

    let selected = output.get_selected_text();

    assert_eq!(selected, None);
}

#[test]
fn test_get_selected_text_skips_line_when_clamped_start_exceeds_end() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "ab".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.selection_start = Some((0, CharIdx::new(4)));
    output.selection_end = Some((0, CharIdx::new(1)));

    let selected = output.get_selected_text();

    assert_eq!(selected, Some("b".to_string()));
}

#[test]
fn test_get_line_content_normal_line() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "hello".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    assert_eq!(output.get_line_content(0), Some("hello".to_string()));
    assert_eq!(output.get_line_content(1), None);
}

#[test]
fn test_get_line_content_task_status_line() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "normal".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.task_status_lines = vec!["task 1".to_string(), "task 2".to_string()];
    // idx=0 → normal line
    assert_eq!(output.get_line_content(0), Some("normal".to_string()));
    // idx=1 → task_status_lines[0], 带 "  " 前缀
    assert_eq!(output.get_line_content(1), Some("  task 1".to_string()));
    // idx=2 → task_status_lines[1]
    assert_eq!(output.get_line_content(2), Some("  task 2".to_string()));
    // idx=3 → 越界
    assert_eq!(output.get_line_content(3), None);
}

#[test]
fn test_total_virtual_line_count() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "a".to_string(),
        style: LineStyle::Normal,
        tool_id: None,
        spans: None,
    });
    output.task_status_lines = vec!["t1".to_string(), "t2".to_string()];
    assert_eq!(output.total_virtual_line_count(), 3);
}

#[test]
fn test_get_selected_text_task_status_only() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "normal line".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.task_status_lines = vec!["pending task".to_string()];
    // 选中 task_status 行（logic_idx=1）
    output.selection_start = Some((1, CharIdx::new(2)));
    output.selection_end = Some((1, CharIdx::new(8)));
    let selected = output.get_selected_text();
    // content is "  pending task", chars [2..8) = "pendin"
    assert_eq!(selected, Some("pendin".to_string()));
}

#[test]
fn test_get_selected_text_spanning_normal_and_task_status() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "abc".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.task_status_lines = vec!["xyz".to_string()];
    // 选中从普通行末尾到 task_status 行开头
    output.selection_start = Some((0, CharIdx::new(1)));
    output.selection_end = Some((1, CharIdx::new(3)));
    let selected = output.get_selected_text();
    // line 0 chars [1..3) = "bc", line 1 content = "  xyz" chars [0..3) = "  x"
    assert_eq!(selected, Some("bc\n  x".to_string()));
}

#[test]
fn test_get_selected_text_markdown_table_uses_rendered_line_offsets() {
    let mut output = OutputArea::new();
    for content in ["| Name | Status |", "| --- | --- |", "| Alice | Done |"] {
        output.push_line(OutputLine {
            content: content.to_string(),
            style: LineStyle::Assistant,
            tool_id: None,
            spans: None,
        });
    }
    let area = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 5,
    };
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf);

    output.start_selection(0, 0, &area);
    output.update_selection(0, 15, &area);

    let selected = output.get_selected_text();

    assert_eq!(selected, Some(" Name  │ Status".to_string()));
}

#[test]
fn test_get_selected_text_uses_rendered_inline_markdown_offsets() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "活动中 Bug（`docs/bug/active.md`）".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    let area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 3,
    };
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf);

    output.start_selection(0, 0, &area);
    output.update_selection(0, 32, &area);

    let selected = output.get_selected_text();

    assert_eq!(
        selected,
        Some("活动中 Bug（docs/bug/active.md）".to_string())
    );
}

#[test]
fn test_get_selected_text_strips_inline_markdown_formatting() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "**bold** and *italic* with `code` plus [link](https://example.com)".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.selection_start = Some((0, CharIdx::new(0)));
    output.selection_end = Some((0, CharIdx::new(output.lines[0].content.chars().count())));

    let selected = output.get_selected_text();

    assert_eq!(
        selected,
        Some("bold and italic with code plus link".to_string())
    );
}

#[test]
fn test_get_selected_text_preserves_unclosed_markdown_marker() {
    let mut output = OutputArea::new();
    output.push_line(OutputLine {
        content: "**unclosed marker".to_string(),
        style: LineStyle::Assistant,
        tool_id: None,
        spans: None,
    });
    output.selection_start = Some((0, CharIdx::new(0)));
    output.selection_end = Some((0, CharIdx::new(output.lines[0].content.chars().count())));

    let selected = output.get_selected_text();

    assert_eq!(selected, Some("**unclosed marker".to_string()));
}
