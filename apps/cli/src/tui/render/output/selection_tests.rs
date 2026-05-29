use super::super::OutputArea;
use crate::tui::render::output::blocks::assistant_message::render_assistant_message;
use crate::tui::render::output::rendered::{
    RenderCtx, RenderedBlock, RenderedDocument, RenderedLine,
};
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;
use ratatui::{buffer::Buffer, layout::Rect, text::Span};
use sdk::CharIdx;

/// 测试辅助：以若干纯文本行填充 document（单 block）。
fn set_plain_lines(output: &mut OutputArea, texts: &[&str]) {
    let lines = texts
        .iter()
        .map(|text| RenderedLine::new(vec![Span::raw(text.to_string())]))
        .collect();
    output.set_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "test".into(),
            lines,
        }],
    });
}

/// 测试辅助：经 assistant_message block 渲染 markdown 文本并写入 document。
/// 复用真实渲染管线，保证 markdown/table 的 plain 与显示偏移一致。
fn set_assistant_markdown(output: &mut OutputArea, text: &str, width: u16) {
    let view = TextBlockView {
        key: "md".into(),
        text: text.to_string(),
        style: SemanticStyle::Normal,
    };
    let block = render_assistant_message("md", &view, &RenderCtx { width });
    output.set_document(RenderedDocument {
        blocks: vec![block],
    });
}

#[test]
fn test_get_selected_text_clamps_start_col_after_line_shrinks() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["短"]);
    output.selection_start = Some((0, CharIdx::new(4)));
    output.selection_end = Some((0, CharIdx::new(6)));

    let selected = output.get_selected_text();

    assert_eq!(selected, None);
}

#[test]
fn test_get_selected_text_skips_line_when_clamped_start_exceeds_end() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["ab"]);
    output.selection_start = Some((0, CharIdx::new(4)));
    output.selection_end = Some((0, CharIdx::new(1)));

    let selected = output.get_selected_text();

    assert_eq!(selected, Some("b".to_string()));
}

#[test]
fn test_get_line_content_normal_line() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["hello"]);
    assert_eq!(output.get_line_content(0), Some("hello".to_string()));
    assert_eq!(output.get_line_content(1), None);
}

#[test]
fn test_get_line_content_task_status_line() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["normal"]);
    output.task_status_lines = vec!["task 1".to_string(), "task 2".to_string()];
    // idx=0 → document 行
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
    set_plain_lines(&mut output, &["a"]);
    output.task_status_lines = vec!["t1".to_string(), "t2".to_string()];
    assert_eq!(output.total_virtual_line_count(), 3);
}

#[test]
fn test_get_selected_text_task_status_only() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["normal line"]);
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
    set_plain_lines(&mut output, &["abc"]);
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
    let area = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 5,
    };
    set_assistant_markdown(
        &mut output,
        "| Name | Status |\n| --- | --- |\n| Alice | Done |",
        area.width.saturating_sub(2),
    );
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
    let area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 3,
    };
    set_assistant_markdown(
        &mut output,
        "活动中 Bug（`docs/bug/active.md`）",
        area.width.saturating_sub(2),
    );
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
    let area = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: 3,
    };
    set_assistant_markdown(
        &mut output,
        "**bold** and *italic* with `code` plus [link](https://example.com)",
        area.width.saturating_sub(2),
    );
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf);
    let plain_len = output
        .document()
        .iter_lines()
        .next()
        .map(|line| line.plain.chars().count())
        .unwrap_or(0);
    output.selection_start = Some((0, CharIdx::new(0)));
    output.selection_end = Some((0, CharIdx::new(plain_len)));

    let selected = output.get_selected_text();

    assert_eq!(
        selected,
        Some("bold and italic with code plus link".to_string())
    );
}

#[test]
fn test_get_selected_text_preserves_unclosed_markdown_marker() {
    let mut output = OutputArea::new();
    let area = Rect {
        x: 0,
        y: 0,
        width: 120,
        height: 3,
    };
    set_assistant_markdown(
        &mut output,
        "**unclosed marker",
        area.width.saturating_sub(2),
    );
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf);
    let plain = output
        .document()
        .iter_lines()
        .next()
        .map(|line| line.plain.clone())
        .unwrap_or_default();
    output.selection_start = Some((0, CharIdx::new(0)));
    output.selection_end = Some((0, CharIdx::new(plain.chars().count())));

    let selected = output.get_selected_text();

    assert_eq!(selected, Some("**unclosed marker".to_string()));
}
