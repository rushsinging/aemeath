use super::render::content_area_for_scrollbar;
use super::OutputArea;
use crate::tui::render::output::rendered::{
    LineAnimation, RenderedBlock, RenderedDocument, RenderedLine,
};
use crate::tui::render::output::status_line::{
    live_status_spinner_fixture, live_status_spinner_fixture_with_frame,
};
use crate::tui::render::output_area::selection::output_selection_view_for_test;
use crate::tui::render::output_area::SCROLLBAR_RESERVE_COLS;
use crate::tui::render::performance::capture;
use crate::tui::render::theme;
use crate::tui::view_model::LiveStatusViewModel;
use crate::tui::view_state::output::OutputViewState;
use ratatui::{buffer::Buffer, layout::Rect, text::Span};
use sdk::CharIdx;
use std::rc::Rc;

fn no_live_status() -> LiveStatusViewModel {
    LiveStatusViewModel::default()
}

#[test]
fn render_records_source_and_visible_document_lines() {
    let mut output = OutputArea::new();
    output.set_plain_document_lines(100);
    let rect = Rect::new(0, 0, 80, 20);
    let view = OutputViewState {
        last_visible_height: 20,
        ..Default::default()
    };
    let mut buffer = Buffer::empty(rect);

    let (_, metrics) = capture(|| {
        output.render(rect, &mut buffer, &view, &no_live_status());
    });

    assert_eq!(metrics.viewport_render_calls, 1);
    assert_eq!(metrics.viewport_source_lines, 100);
    assert_eq!(metrics.viewport_visible_lines, 20);
}

#[test]
fn render_only_queries_visible_document_lines() {
    let mut output = OutputArea::new();
    output.replace_document(RenderedDocument::new(
        (0..1_000)
            .map(|index| RenderedBlock {
                block_id: format!("block-{index}"),
                lines: Rc::new(vec![RenderedLine::from_plain(format!("line {index}"))]),
            })
            .collect(),
    ));
    let rect = Rect::new(0, 0, 80, 20);
    let view = OutputViewState {
        last_visible_height: 20,
        ..Default::default()
    };
    let mut buffer = Buffer::empty(rect);

    output.render(rect, &mut buffer, &view, &no_live_status());

    assert_eq!(output.last_document_line_visits(), 20);
    assert_eq!(
        output.screen_line_map.first().map(|entry| entry.0),
        Some(980)
    );
    assert_eq!(
        output.screen_line_map.last().map(|entry| entry.0),
        Some(999)
    );
}

#[test]
fn test_render_reserves_scrollbar_column_and_wraps_long_lines() {
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![
                RenderedLine::new(vec![Span::raw("line 1")]),
                RenderedLine::new(vec![Span::raw("1234567")]),
                RenderedLine::new(vec![Span::raw("line 2")]),
            ]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 6, 2);
    let view = OutputViewState {
        last_visible_height: 2,
        ..Default::default()
    };
    let mut buf = Buffer::empty(area_rect);

    area.render(area_rect, &mut buf, &view, &no_live_status());

    assert_ne!(
        buf[(5, 0)].symbol(),
        "6",
        "最右列预留给滚动条，不应渲染正文"
    );
    assert_eq!(buf[(0, 1)].symbol(), "l", "多行文档应继续渲染下一条逻辑行");
}

#[test]
fn test_content_area_width_matches_output_document_width_when_scrollbar_visible() {
    let area_rect = Rect::new(0, 0, 80, 10);

    let content_area = content_area_for_scrollbar(area_rect, true);

    assert_eq!(
        content_area.width,
        area_rect
            .width
            .saturating_sub(SCROLLBAR_RESERVE_COLS)
            .max(1),
        "输出文档预换行宽度必须等于 Paragraph 实际渲染宽度，避免二次折行"
    );
}

#[test]
fn test_render_document_paints_spans_and_overlays_selection() {
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![RenderedLine::new(vec![Span::raw("hello")])]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 10, 3);
    let view = OutputViewState {
        last_visible_height: 3,
        ..output_selection_view_for_test((0, CharIdx::new(0)), (0, CharIdx::new(3)))
    };
    let mut buf = Buffer::empty(area_rect);
    area.render(area_rect, &mut buf, &view, &no_live_status());

    assert_eq!(buf[(0, 0)].bg, theme::SELECTION_BG);
    assert_eq!(buf[(2, 0)].bg, theme::SELECTION_BG);
    assert_ne!(buf[(3, 0)].bg, theme::SELECTION_BG);
}

#[test]
fn test_output_area_paints_fill_style_for_short_and_empty_lines() {
    let fill = ratatui::style::Style::default().bg(ratatui::style::Color::Blue);
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "filled".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("hi").with_fill_style(fill),
                RenderedLine::empty().with_fill_style(fill),
            ]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 8, 3);
    let view = OutputViewState {
        last_visible_height: 3,
        ..Default::default()
    };
    let mut buf = Buffer::empty(area_rect);

    area.render(area_rect, &mut buf, &view, &no_live_status());

    for x in 0..6 {
        assert_eq!(buf[(x, 0)].bg, ratatui::style::Color::Blue);
        assert_eq!(buf[(x, 1)].bg, ratatui::style::Color::Blue);
    }
    // col 6..8 是右侧呼吸空间，不填充
    assert_ne!(buf[(7, 0)].bg, ratatui::style::Color::Blue);
}

#[test]
fn test_output_area_selection_overrides_fill_style_on_text_cells() {
    let fill = ratatui::style::Style::default().bg(ratatui::style::Color::Blue);
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "filled".into(),
            lines: Rc::new(vec![RenderedLine::from_plain("hello").with_fill_style(fill)]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 8, 2);
    let view = OutputViewState {
        last_visible_height: 2,
        ..output_selection_view_for_test((0, CharIdx::new(0)), (0, CharIdx::new(2)))
    };
    let mut buf = Buffer::empty(area_rect);

    area.render(area_rect, &mut buf, &view, &no_live_status());

    assert_eq!(buf[(0, 0)].bg, theme::SELECTION_BG);
    assert_eq!(buf[(1, 0)].bg, theme::SELECTION_BG);
    assert_eq!(buf[(2, 0)].bg, ratatui::style::Color::Blue);
    assert_eq!(buf[(5, 0)].bg, ratatui::style::Color::Blue);
    // col 6..8 是右侧呼吸空间
    assert_ne!(buf[(7, 0)].bg, ratatui::style::Color::Blue);
}

#[test]
fn test_render_document_with_gutter_offsets_selection_and_skips_gutter() {
    // 带 gutter（"✓ "，宽 2）的行，plain="hello"。
    let mut line =
        RenderedLine::with_plain(vec![Span::raw("✓ "), Span::raw("hello")], "hello".into());
    line.gutter_cols = 2;
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![line]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    // 选中 plain 字符 [0,3) = "hel"
    let view = OutputViewState {
        last_visible_height: 3,
        ..output_selection_view_for_test((0, CharIdx::new(0)), (0, CharIdx::new(3)))
    };
    let area_rect = Rect::new(0, 0, 12, 3);
    let mut buf = Buffer::empty(area_rect);
    area.render(area_rect, &mut buf, &view, &no_live_status());

    // gutter 占屏幕列 0..2，绝不高亮。
    assert_ne!(buf[(0, 0)].bg, theme::SELECTION_BG, "gutter 列不选中");
    assert_ne!(buf[(1, 0)].bg, theme::SELECTION_BG, "gutter 列不选中");
    // 内容从屏幕列 2 起，"hel" 高亮 → 列 2,3,4。
    assert_eq!(buf[(2, 0)].bg, theme::SELECTION_BG, "内容首字符 h 高亮");
    assert_eq!(buf[(4, 0)].bg, theme::SELECTION_BG, "内容第三字符 l 高亮");
    assert_ne!(buf[(5, 0)].bg, theme::SELECTION_BG, "第四字符 l 不在选区");
}

#[test]
fn test_render_user_message_paints_full_visible_line_background() {
    let mut line = RenderedLine::with_plain(
        vec![
            Span::styled("> ", ratatui::style::Style::default().fg(theme::USER)),
            Span::styled(
                "hello",
                ratatui::style::Style::default()
                    .fg(theme::USER)
                    .bg(theme::USER_BG),
            ),
        ],
        "hello".into(),
    )
    .with_fill_style(ratatui::style::Style::default().bg(theme::USER_BG));
    line.gutter_cols = 2;
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "u".into(),
            lines: Rc::new(vec![line]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 12, 2);
    let view = OutputViewState {
        last_visible_height: 2,
        ..Default::default()
    };
    let mut buf = Buffer::empty(area_rect);

    area.render(area_rect, &mut buf, &view, &no_live_status());

    assert_eq!(buf[(0, 0)].bg, theme::USER_BG, "gutter 也应有用户消息背景");
    assert_eq!(buf[(0, 0)].fg, theme::USER, "gutter 应使用深色用户消息前景");
    assert_eq!(buf[(2, 0)].bg, theme::USER_BG, "正文应有用户消息背景");
    assert_eq!(buf[(2, 0)].fg, theme::USER, "正文应使用深色用户消息前景");
    assert_eq!(buf[(9, 0)].bg, theme::USER_BG, "行尾空白也应有用户消息背景");
    // col 10..12 是右侧呼吸空间
    assert_ne!(buf[(11, 0)].bg, theme::USER_BG);
    assert_ne!(buf[(0, 1)].bg, theme::USER_BG, "非用户消息行不应被背景污染");
}

#[test]
fn test_click_on_gutter_line_maps_to_content_char() {
    let mut line =
        RenderedLine::with_plain(vec![Span::raw("✓ "), Span::raw("hello")], "hello".into());
    line.gutter_cols = 2;
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![line]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 12, 3);
    let view = OutputViewState {
        last_visible_height: 3,
        ..Default::default()
    };
    let mut buf = Buffer::empty(area_rect);
    area.render(area_rect, &mut buf, &view, &no_live_status());

    // 点击屏幕列 2（内容 "h"）→ plain 字符 0；拖到列 5（内容 "l" 之后）→ plain 3。
    // 经只读换算 screen_to_anchor 折算锚点后直接置选区镜像（widget start/update_selection
    // 已删除，选区真相迁至 view_state）。
    let s = area
        .screen_to_anchor(0, 2, &area_rect, &no_live_status())
        .unwrap();
    let e = area
        .screen_to_anchor(0, 5, &area_rect, &no_live_status())
        .unwrap();
    let view = output_selection_view_for_test(s, e);
    assert_eq!(
        area.selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some("hel")
    );

    // 点击 gutter 区间（列 0）→ 映射到 plain 字符 0，不偏移。
    let s = area
        .screen_to_anchor(0, 0, &area_rect, &no_live_status())
        .unwrap();
    let e = area
        .screen_to_anchor(0, 4, &area_rect, &no_live_status())
        .unwrap();
    let view = output_selection_view_for_test(s, e);
    assert_eq!(
        area.selected_text_for_view(&view, &no_live_status())
            .as_deref(),
        Some("he"),
        "点击 gutter 钳到 plain 0，拖到列 4 选到内容字符 2"
    );
}

#[test]
fn visible_running_tool_gutter_animates_without_replacing_document() {
    let mut line = RenderedLine::with_plain(
        vec![
            Span::styled(
                "● ",
                ratatui::style::Style::default().fg(theme::TOOL_RUNNING),
            ),
            Span::raw("Bash"),
        ],
        "Bash".into(),
    );
    line.gutter_cols = 2;
    line.animation = Some(LineAnimation::RunningToolMarker);
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument::new(vec![RenderedBlock {
        block_id: "running-tool".into(),
        lines: Rc::new(vec![line]),
    }]));
    let rect = Rect::new(0, 0, 12, 2);
    let view = OutputViewState {
        last_visible_height: 2,
        ..Default::default()
    };
    let mut frame_zero = Buffer::empty(rect);
    let mut frame_four = Buffer::empty(rect);
    let live_zero = live_status_spinner_fixture_with_frame("Thinking", 0);
    let live_four = live_status_spinner_fixture_with_frame("Thinking", 4);

    area.render(rect, &mut frame_zero, &view, &live_zero);
    area.render(rect, &mut frame_four, &view, &live_four);

    assert_eq!(frame_zero[(0, 0)].symbol(), "●");
    assert_eq!(frame_four[(0, 0)].symbol(), "○");
    assert_eq!(area.document().total_lines(), 1);
}

#[test]
fn test_render_spinner_does_not_overflow_into_scrollbar_gap_narrow_terminal() {
    let mut area = OutputArea::new();
    // 文档 10 行 + visible=3 → needs_scrollbar=true → content_area.width=30-3=27
    let doc_lines: Vec<RenderedLine> = (0..10)
        .map(|i| RenderedLine::new(vec![Span::raw(format!("line {i}"))]))
        .collect();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(doc_lines),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 30, 3);
    let view = OutputViewState {
        last_visible_height: 3,
        ..Default::default()
    };
    let live_status = live_status_spinner_fixture("Synthesizing", 24, 2, Some("Thinking"));
    let mut buf = Buffer::empty(area_rect);

    area.render(area_rect, &mut buf, &view, &live_status);

    // content_area 宽度 = area.width - SCROLLBAR_RESERVE_COLS = 27
    let content_width = (area_rect.width as usize).saturating_sub(SCROLLBAR_RESERVE_COLS as usize);
    // 断言：scrollbar 间隙列（content_width .. area_width - 1）应当为空格或 scrollbar 字符，
    // 不应出现 spinner 文本片段（'i' 'n' 'g' '(' 等 ASCII 文本）。
    for row in 0..area_rect.height {
        for col in content_width..(area_rect.width as usize - 1) {
            let cell = &buf[(col as u16, row)];
            let sym = cell.symbol();
            let first = sym.chars().next().unwrap_or(' ');
            let is_overflow_char = sym.len() == 1 && first.is_ascii_alphanumeric();
            assert!(
                !is_overflow_char,
                "scrollbar gap row={row} col={col} 出现 spinner 文本片段: {sym:?} (content_width={content_width})"
            );
        }
    }
}

#[test]
fn test_render_spinner_does_not_overflow_into_scrollbar_gap_very_narrow_terminal() {
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![RenderedLine::new(vec![Span::raw("doc")])]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    // 更窄：width=18，content_width=15
    let area_rect = Rect::new(0, 0, 18, 2);
    let view = OutputViewState {
        last_visible_height: 2,
        ..Default::default()
    };
    let live_status = live_status_spinner_fixture("Synthesizing", 24, 2, Some("Thinking"));
    let mut buf = Buffer::empty(area_rect);

    area.render(area_rect, &mut buf, &view, &live_status);

    let content_width = (area_rect.width as usize).saturating_sub(SCROLLBAR_RESERVE_COLS as usize);
    for row in 0..area_rect.height {
        for col in content_width..(area_rect.width as usize - 1) {
            let cell = &buf[(col as u16, row)];
            let sym = cell.symbol();
            let first = sym.chars().next().unwrap_or(' ');
            let is_overflow_char = sym.len() == 1 && first.is_ascii_alphanumeric();
            assert!(
                !is_overflow_char,
                "very-narrow: scrollbar gap row={row} col={col} spinner 溢出: {sym:?}"
            );
        }
    }
}

/// 输出区清除必须把**显式** `bg=BASE` 写进 buffer，绝不把 `Color::Reset`
/// 送到终端——crossterm 对 Reset bg 处理不正确，ghostty 系终端会保留旧背景
/// （表现为退格后蓝色光标格残留在屏幕上、持续累积）。与 input area 的
/// 既有修复（`render/input_area/render.rs` 的 `set_style(bg(BASE))`）同一模式。
#[test]
fn test_render_clears_viewport_with_explicit_base_bg_never_reset() {
    let mut area = OutputArea::new();
    area.replace_document(RenderedDocument {
        blocks: vec![RenderedBlock {
            block_id: "short".into(),
            lines: Rc::new(vec![RenderedLine::new(vec![Span::raw("hi")])]),
        }],
        root_group_block_counts: Vec::new(),
        block_line_ends: Vec::new(),
    });
    let area_rect = Rect::new(0, 0, 8, 4);
    let view = OutputViewState {
        last_visible_height: 4,
        ..Default::default()
    };
    let mut buf = Buffer::empty(area_rect);
    // 模拟上一帧残留：整行脏的 ACCENT 蓝色 bg（Type something 光标色）
    for x in 0..area_rect.width {
        buf[(x, 3)].set_bg(theme::ACCENT);
    }

    area.render(area_rect, &mut buf, &view, &no_live_status());

    for y in 0..area_rect.height {
        for x in 0..area_rect.width {
            assert_eq!(
                buf[(x, y)].bg,
                theme::BASE,
                "cell ({x},{y}) 清除后必须携带显式 BASE bg，Color::Reset 会在终端残留"
            );
        }
    }
}

#[test]
fn render_keeps_table_plain_identical_when_line_is_window_first_row() {
    // 窗口首行曾对表格 plain 做 trim 归一化并写回 rendered_content，
    // 导致同一行滚到窗口首行后复制文本/选区字符偏移漂移。
    const TABLE_LINE: &str = " a │ b ";
    let mut output = OutputArea::new();
    output.replace_document(RenderedDocument::new(vec![RenderedBlock {
        block_id: "table".to_string(),
        lines: Rc::new(
            (0..30)
                .map(|index| {
                    RenderedLine::from_plain(if index == 20 { TABLE_LINE } else { "plain" })
                })
                .collect(),
        ),
    }]));

    let rect = Rect::new(0, 0, 80, 10);
    let mut buffer = Buffer::empty(rect);

    // 表格行作为窗口首行：start = 20。
    let first_row_view = OutputViewState {
        auto_scroll: false,
        last_visible_height: 10,
        scroll_offset: 0,
        ..Default::default()
    };
    output.render(rect, &mut buffer, &first_row_view, &no_live_status());
    let as_first_row = output.rendered_line_content.get(&20).cloned();

    // 表格行位于窗口中段：start = 15，表格行是第 6 个可见行。
    let middle_view = OutputViewState {
        auto_scroll: false,
        last_visible_height: 10,
        scroll_offset: 5,
        ..Default::default()
    };
    output.render(rect, &mut buffer, &middle_view, &no_live_status());
    let as_middle_row = output.rendered_line_content.get(&20).cloned();

    assert_eq!(as_first_row.as_deref(), Some(TABLE_LINE));
    assert_eq!(as_first_row, as_middle_row);
}

/// 构造三行表格文档：表头行、分隔行、数据行（均带表格元数据）。
fn table_document() -> RenderedDocument {
    RenderedDocument::new(vec![RenderedBlock {
        block_id: "table".to_string(),
        lines: Rc::new(vec![
            RenderedLine::new(vec![Span::raw(" Name │ Value ")]).with_table_row_fixture(0),
            RenderedLine::new(vec![Span::raw("──────┼───────")]),
            RenderedLine::new(vec![Span::raw(" foo  │ bar   ")]).with_table_row_fixture(1),
        ]),
    }])
}

fn table_view() -> OutputViewState {
    OutputViewState {
        auto_scroll: false,
        last_visible_height: 10,
        ..Default::default()
    }
}

#[test]
fn table_cell_at_snaps_click_to_cell() {
    use crate::tui::view_state::output::CellAnchor;

    let mut output = OutputArea::new();
    output.replace_document(table_document());
    let rect = Rect::new(0, 0, 80, 10);
    let view = table_view();
    let mut buffer = Buffer::empty(rect);
    output.render(rect, &mut buffer, &view, &no_live_status());

    // 数据行第 0 列（'f' 位于屏幕列 1）。
    assert_eq!(
        output.table_cell_at(2, 1, &rect, &no_live_status()),
        Some(CellAnchor { line: 2, col: 0 })
    );
    // 分隔符归右侧列。
    assert_eq!(
        output.table_cell_at(2, 6, &rect, &no_live_status()),
        Some(CellAnchor { line: 2, col: 1 })
    );
    // 表头属于 data_row 0。
    assert_eq!(
        output.table_cell_at(0, 1, &rect, &no_live_status()),
        Some(CellAnchor { line: 0, col: 0 })
    );
    // 分隔行不是数据行。
    assert_eq!(output.table_cell_at(1, 3, &rect, &no_live_status()), None);
}

#[test]
fn cell_selection_highlights_only_selected_columns() {
    use crate::tui::view_state::output::CellAnchor;

    let mut output = OutputArea::new();
    output.replace_document(table_document());
    let rect = Rect::new(0, 0, 80, 10);
    let mut view = table_view();
    view.begin_cell_selection(CellAnchor { line: 2, col: 1 });
    let mut buffer = Buffer::empty(rect);
    output.render(rect, &mut buffer, &view, &no_live_status());

    // 选中的第二列：'b' 位于屏幕列 8。
    assert_eq!(buffer[(8, 2)].bg, theme::SELECTION_BG);
    // 未选中的第一列 'f' 保持原底色。
    assert_ne!(buffer[(1, 2)].bg, theme::SELECTION_BG);
}

#[test]
fn cell_selection_highlights_wrapped_continuation_rows() {
    use crate::tui::view_state::output::CellAnchor;

    let mut output = OutputArea::new();
    output.replace_document(RenderedDocument::new(vec![RenderedBlock {
        block_id: "wrapped".to_string(),
        lines: Rc::new(vec![
            RenderedLine::new(vec![Span::raw(" abcdef ")]).with_table_row_fixture(0),
            RenderedLine::new(vec![Span::raw(" cdef   ")]).with_table_row_fixture(0),
            RenderedLine::new(vec![Span::raw(" x │ y ")]).with_table_row_fixture(1),
        ]),
    }]));
    let rect = Rect::new(0, 0, 80, 10);
    let mut view = table_view();
    view.begin_cell_selection(CellAnchor { line: 0, col: 0 });
    let mut buffer = Buffer::empty(rect);
    output.render(rect, &mut buffer, &view, &no_live_status());

    // 折行续行与首行同属数据行 0，二者都应是选中底色。
    assert_eq!(buffer[(1, 0)].bg, theme::SELECTION_BG);
    assert_eq!(buffer[(1, 1)].bg, theme::SELECTION_BG);
    // 另一数据行不受影响。
    assert_ne!(buffer[(1, 2)].bg, theme::SELECTION_BG);
}
