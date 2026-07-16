use super::render::content_area_for_scrollbar;
use super::OutputArea;
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
use crate::tui::render::output::status_line::live_status_spinner_fixture;
use crate::tui::render::output_area::selection::output_selection_view_for_test;
use crate::tui::render::output_area::SCROLLBAR_RESERVE_COLS;
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
    assert_eq!(
        buf[(9, 0)].bg,
        theme::USER_BG,
        "行尾空白也应有用户消息背景"
    );
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
