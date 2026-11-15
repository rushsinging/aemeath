use super::super::OutputArea;
use super::output_selection_view_for_test;
use crate::tui::render::output::blocks::assistant_message::render_assistant_message;
use crate::tui::render::output::rendered::{
    RenderCtx, RenderedBlock, RenderedDocument, RenderedLine,
};
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;
use crate::tui::view_model::{LiveStatusViewModel, SpinnerLineView};
use crate::tui::view_state::output::OutputViewState;
use ratatui::{buffer::Buffer, layout::Rect, text::Span};
use sdk::CharIdx;
use std::rc::Rc;
fn live_status(task_lines: Vec<&str>) -> LiveStatusViewModel {
    LiveStatusViewModel {
        spinner: Some(SpinnerLineView {
            frame: 0,
            verb: "Thinking".to_string(),
            elapsed_secs: 0,
            phase_elapsed_secs: Some(0),
            phase_text: None,
            detail_text: None,
        }),
        queued_lines: Vec::new(),
        task_lines: task_lines.into_iter().map(str::to_string).collect(),
        compact_progress: None,
    }
}
fn no_live_status() -> LiveStatusViewModel {
    LiveStatusViewModel::default()
}
/// Test helper for no-live-status render paths; callers with spinner/queued/task lines must
/// compute visible height through the same projection used by App.
fn render_view_for_area(area: &Rect) -> OutputViewState {
    OutputViewState {
        last_visible_height: area.height as usize,
        ..Default::default()
    }
}
/// 测试辅助：以若干纯文本行填充 document（单 block）。
fn set_plain_lines(output: &mut OutputArea, texts: &[&str]) {
    let lines: Vec<RenderedLine> = texts
        .iter()
        .map(|text| RenderedLine::new(vec![Span::raw(text.to_string())]))
        .collect();
    output.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "test".into(),
            lines: Rc::new(lines),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
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
    let block = render_assistant_message("md", &view, &RenderCtx::for_width(width));
    output.replace_document(RenderedDocument {
        blocks: vec![block],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
}
#[test]
fn test_get_selected_text_clamps_start_col_after_line_shrinks() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["短"]);
    let view = output_selection_view_for_test((0, CharIdx::new(4)), (0, CharIdx::new(6)));
    let selected = output.selected_text_for_view(&view, &no_live_status());
    assert_eq!(selected, None);
}
#[test]
fn test_get_selected_text_skips_line_when_clamped_start_exceeds_end() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["ab"]);
    let view = output_selection_view_for_test((0, CharIdx::new(4)), (0, CharIdx::new(1)));
    let selected = output.selected_text_for_view(&view, &no_live_status());
    assert_eq!(selected, Some("b".to_string()));
}
#[test]
fn test_get_line_content_normal_line() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["hello"]);
    assert_eq!(
        output.get_line_content(0, &no_live_status()),
        Some("hello".to_string())
    );
    assert_eq!(output.get_line_content(1, &no_live_status()), None);
}
#[test]
fn test_get_line_content_task_status_line() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["normal"]);
    let live_status = live_status(vec!["task 1", "task 2"]);
    // idx=0 → document 行
    assert_eq!(
        output.get_line_content(0, &live_status),
        Some("normal".to_string())
    );
    // idx=1 → task_lines[0], 带 "  " 前缀
    assert_eq!(
        output.get_line_content(1, &live_status),
        Some("  task 1".to_string())
    );
    // idx=2 → task_lines[1]
    assert_eq!(
        output.get_line_content(2, &live_status),
        Some("  task 2".to_string())
    );
    // idx=3 → 越界
    assert_eq!(output.get_line_content(3, &live_status), None);
}
#[test]
fn test_total_virtual_line_count() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["a"]);
    let live_status = live_status(vec!["t1", "t2"]);
    assert_eq!(output.total_virtual_line_count(&live_status), 3);
}
#[test]
fn test_get_selected_text_task_status_only() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["normal line"]);
    let live_status = live_status(vec!["pending task"]);
    // 选中 task_status 行（logic_idx=1）
    let view = output_selection_view_for_test((1, CharIdx::new(2)), (1, CharIdx::new(8)));
    let selected = output.selected_text_for_view(&view, &live_status);
    // content is "  pending task", chars [2..8) = "pendin"
    assert_eq!(selected, Some("pendin".to_string()));
}
#[test]
fn test_get_selected_text_spanning_normal_and_task_status() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &["abc"]);
    let live_status = live_status(vec!["xyz"]);
    // 选中从普通行末尾到 task_status 行开头
    let view = output_selection_view_for_test((0, CharIdx::new(1)), (1, CharIdx::new(3)));
    let selected = output.selected_text_for_view(&view, &live_status);
    // line 0 chars [1..3) = "bc", line 1 content = "  xyz" chars [0..3) = "  x"
    assert_eq!(selected, Some("bc\n  x".to_string()));
}
#[test]
fn test_get_selected_text_markdown_table_uses_rendered_line_offsets() {
    let mut output = OutputArea::new();
    let area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 5,
    };
    set_assistant_markdown(
        &mut output,
        "| Name | Status |\n| --- | --- |\n| Alice | Done |",
        area.width.saturating_sub(2),
    );
    let mut buf = Buffer::empty(area);
    output.render(
        area,
        &mut buf,
        &render_view_for_area(&area),
        &no_live_status(),
    );
    // 经只读换算 screen_to_anchor 取屏幕坐标对应的逻辑锚点，直接置选区镜像（替代
    // 已删除的 widget start/update_selection；选区真相迁至 view_state）。
    let start = output
        .screen_to_anchor(0, 0, &area, &no_live_status())
        .unwrap();
    let end = output
        .screen_to_anchor(0, 15, &area, &no_live_status())
        .unwrap();
    let view = output_selection_view_for_test(start, end);
    let selected = output.selected_text_for_view(&view, &no_live_status());
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
    output.render(
        area,
        &mut buf,
        &render_view_for_area(&area),
        &no_live_status(),
    );
    let start = output
        .screen_to_anchor(0, 0, &area, &no_live_status())
        .unwrap();
    let end = output
        .screen_to_anchor(0, 32, &area, &no_live_status())
        .unwrap();
    let view = output_selection_view_for_test(start, end);
    let selected = output.selected_text_for_view(&view, &no_live_status());
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
    output.render(
        area,
        &mut buf,
        &render_view_for_area(&area),
        &no_live_status(),
    );
    let plain_len = output
        .document()
        .iter_lines()
        .next()
        .map(|line| line.plain.chars().count())
        .unwrap_or(0);
    let view = output_selection_view_for_test((0, CharIdx::new(0)), (0, CharIdx::new(plain_len)));
    let selected = output.selected_text_for_view(&view, &no_live_status());
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
    output.render(
        area,
        &mut buf,
        &render_view_for_area(&area),
        &no_live_status(),
    );
    let plain = output
        .document()
        .iter_lines()
        .next()
        .map(|line| line.plain.clone())
        .unwrap_or_default();
    let view = output_selection_view_for_test(
        (0, CharIdx::new(0)),
        (0, CharIdx::new(plain.chars().count())),
    );
    let selected = output.selected_text_for_view(&view, &no_live_status());
    assert_eq!(selected, Some("**unclosed marker".to_string()));
}
/// 渲染若干纯文本行并回填 screen_line_map，供坐标换算测试。
fn rendered_plain(texts: &[&str], width: u16) -> (OutputArea, Rect) {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, texts);
    let area = Rect {
        x: 0,
        y: 0,
        width,
        height: texts.len() as u16 + 1,
    };
    let mut buf = Buffer::empty(area);
    output.render(
        area,
        &mut buf,
        &render_view_for_area(&area),
        &no_live_status(),
    );
    (output, area)
}
#[test]
fn test_screen_to_anchor_maps_row_col_to_logic_char() {
    let (output, area) = rendered_plain(&["hello", "世界"], 40);
    // 正常路径：第 0 行第 3 列 → (逻辑行 0, plain char 3)。
    assert_eq!(
        output.screen_to_anchor(0, 3, &area, &no_live_status()),
        Some((0, CharIdx::new(3)))
    );
    // CJK：第 1 行第 2 屏幕列（"世"占 2 列）落在第 1 个字符。
    assert_eq!(
        output.screen_to_anchor(1, 2, &area, &no_live_status()),
        Some((1, CharIdx::new(1)))
    );
}
#[test]
fn test_screen_to_anchor_returns_none_when_row_out_of_range() {
    let (output, area) = rendered_plain(&["abc"], 40);
    // 错误/边界路径：屏幕行超出 screen_line_map 返回 None（不改任何状态）。
    assert_eq!(
        output.screen_to_anchor(5, 0, &area, &no_live_status()),
        None
    );
}
#[test]
fn test_screen_to_anchor_gutter_columns_map_to_plain_zero() {
    let mut line =
        RenderedLine::with_plain(vec![Span::raw("✓ "), Span::raw("hello")], "hello".into());
    line.gutter_cols = 2;
    let mut output = OutputArea::new();
    output.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "g".into(),
            lines: Rc::new(vec![line]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    output.render(
        area,
        &mut buf,
        &render_view_for_area(&area),
        &no_live_status(),
    ); // #63：点击 gutter 列（0/1）补偿后映射到 plain 字符 0。
    assert_eq!(
        output.screen_to_anchor(0, 0, &area, &no_live_status()),
        Some((0, CharIdx::new(0)))
    );
    assert_eq!(
        output.screen_to_anchor(0, 1, &area, &no_live_status()),
        Some((0, CharIdx::new(0)))
    );
    // 内容列 2（"h"）→ plain 字符 0；列 4（"l"）→ plain 字符 2。
    assert_eq!(
        output.screen_to_anchor(0, 2, &area, &no_live_status()),
        Some((0, CharIdx::new(0)))
    );
    assert_eq!(
        output.screen_to_anchor(0, 4, &area, &no_live_status()),
        Some((0, CharIdx::new(2)))
    );
}
#[test]
fn test_word_bounds_at_returns_word_half_open_range() {
    let (output, area) = rendered_plain(&["foo bar_baz qux"], 40);
    // 正常路径：点击 "bar_baz" 内 → 半开区间 [4, 11)（下划线视作 word-char）。
    assert_eq!(
        output.word_bounds_at(0, 6, &area, &no_live_status()),
        Some((0, CharIdx::new(4), CharIdx::new(11)))
    );
    // 边界：点击空白（非 word-char）→ 单字符词 [3, 4)。
    assert_eq!(
        output.word_bounds_at(0, 3, &area, &no_live_status()),
        Some((0, CharIdx::new(3), CharIdx::new(4)))
    );
    // 错误路径：行超界返回 None。
    assert_eq!(output.word_bounds_at(9, 0, &area, &no_live_status()), None);
}
#[test]
fn test_last_visible_anchor_returns_last_screen_line_end() {
    let (output, _area) = rendered_plain(&["abc", "de"], 40);
    // 正常路径：最后一屏幕行对应逻辑行 1，末尾 char_end 为该行字符数。
    let last = output.last_visible_anchor();
    assert_eq!(last.map(|(l, _)| l), Some(1));
    // 边界：空 document 时 screen_line_map 为空 → None。
    let empty = OutputArea::new();
    assert_eq!(empty.last_visible_anchor(), None);
}

/// 测试辅助：构造单元格矩形选区（坐标为逻辑行号 + 列序号）。
fn cell_view(anchor: (usize, usize), focus: (usize, usize)) -> OutputViewState {
    use crate::tui::view_state::output::CellAnchor;
    let mut view = OutputViewState::default();
    view.begin_cell_selection(CellAnchor {
        line: anchor.0,
        col: anchor.1,
    });
    view.update_cell_selection(CellAnchor {
        line: focus.0,
        col: focus.1,
    });
    view
}

fn ascii_cell(len: usize) -> String {
    ('A'..='Z').cycle().take(len).collect()
}

#[test]
fn cell_selection_copies_single_cell_without_separator() {
    let mut output = OutputArea::new();
    set_assistant_markdown(
        &mut output,
        "| Name | Value |\n| --- | --- |\n| foo | bar |",
        80,
    );
    let view = cell_view((2, 1), (2, 1));
    assert_eq!(
        output
            .selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some("bar")
    );
}

#[test]
fn cell_selection_copies_rectangle_as_tab_separated_text() {
    let mut output = OutputArea::new();
    set_assistant_markdown(
        &mut output,
        "| Name | Value |\n| --- | --- |\n| foo | bar |",
        80,
    );
    let view = cell_view((0, 0), (2, 1));
    assert_eq!(
        output
            .selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some("Name\tValue\nfoo\tbar")
    );
}

#[test]
fn cell_selection_copies_single_column_across_rows() {
    let mut output = OutputArea::new();
    set_assistant_markdown(
        &mut output,
        "| Name | Value |\n| --- | --- |\n| foo | bar |",
        80,
    );
    let view = cell_view((0, 0), (2, 0));
    assert_eq!(
        output
            .selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some("Name\nfoo")
    );
}

#[test]
fn cell_selection_joins_wrapped_physical_rows_back_into_one_cell() {
    let long = ascii_cell(78);
    let mut output = OutputArea::new();
    set_assistant_markdown(
        &mut output,
        &format!("| {long} | b |\n| --- | --- |\n| x | y |"),
        80,
    );
    // 第一列在 80 宽下会折成多个物理行，它们共享同一 data_row。
    let view = cell_view((0, 0), (0, 0));
    assert_eq!(
        output
            .selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some(long.as_str())
    );
}

#[test]
fn cell_selection_ignores_rows_without_table_row_metadata() {
    let mut output = OutputArea::new();
    set_plain_lines(&mut output, &[" a │ b "]);
    let view = cell_view((0, 0), (0, 1));
    assert_eq!(
        output.selected_text_for_view(&view, &no_live_status()),
        None
    );
}

#[test]
fn cell_selection_keeps_separate_tables_on_separate_rows() {
    // 两个表格块都用块内 data_row 0/1：分组必须按块隔离，否则两表会被拼成同一行。
    let table = |prefix: &str, block_id: &str| RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(vec![
            RenderedLine::new(vec![Span::raw(format!(" {prefix}1 │ {prefix}2 "))])
                .with_table_row_fixture(0),
            RenderedLine::new(vec![Span::raw("────┼────")]),
            RenderedLine::new(vec![Span::raw(format!(" {prefix}a │ {prefix}b "))])
                .with_table_row_fixture(1),
        ]),
    };
    let mut output = OutputArea::new();
    output.replace_document(RenderedDocument {
        blocks: vec![table("A", "t1"), table("B", "t2")],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });

    let view = cell_view((0, 0), (5, 1));
    assert_eq!(
        output
            .selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some("A1\tA2\nAa\tAb\nB1\tB2\nBa\tBb")
    );
}
