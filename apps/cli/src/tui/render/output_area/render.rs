use ratatui::{
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use sdk::CharIdx;

use super::OutputArea;
use crate::tui::render::display::safe_text::str_display_width;
use crate::tui::render::output::selection_overlay::{apply_selection_overlay, SelRange};
use crate::tui::view_model::LiveStatusViewModel;
use crate::tui::view_state::output::{OutputViewState, SelectionAnchor};

/// 输出区内容为滚动条预留的列数：滚动条本身（1 列）+ 与内容之间的间距（2 列）。
/// 此常量同时用于文档预换行宽度（`output_document_width`）与实际渲染区域
/// （`content_area_for_scrollbar`），两侧 MUST 保持同步，避免二次折行。
pub(crate) const SCROLLBAR_RESERVE_COLS: u16 = 3;

impl OutputArea {
    /// 渲染输出区域
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        view: &OutputViewState,
        live_status: &LiveStatusViewModel,
    ) {
        if area.height == 0 {
            return;
        }

        let new_width = (area.width as usize).saturating_sub(2);
        if new_width != self.term_width {
            self.term_width = new_width;
        }

        let spinner_line = live_status
            .spinner
            .as_ref()
            .map(|spinner| self.build_spinner_line(spinner));

        let visible_lines = view.last_visible_height;
        let total_lines = self.document.total_lines();
        let needs_scrollbar = total_lines > visible_lines;
        let content_area = content_area_for_scrollbar(area, needs_scrollbar);
        let (start, end) = visible_range(
            total_lines,
            visible_lines,
            view.auto_scroll,
            view.scroll_offset,
        );

        clear_area(area, buf);

        let document_lines = self.document.iter_lines().collect::<Vec<_>>();
        let first_visible_doc_line_plain = document_lines
            .get(start)
            .map(|line| diagnostic_plain(&line.plain))
            .unwrap_or_default();
        let last_visible_doc_line_plain = end
            .checked_sub(1)
            .and_then(|idx| document_lines.get(idx))
            .map(|line| diagnostic_plain(&line.plain))
            .unwrap_or_default();
        let last_doc_line_plain = document_lines
            .last()
            .map(|line| diagnostic_plain(&line.plain))
            .unwrap_or_default();
        let visible_overwide_lines = document_lines
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .filter(|line| str_display_width(&line.plain) > content_area.width as usize)
            .count();
        let max_visible_line_width = document_lines
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .map(|line| str_display_width(&line.plain))
            .max()
            .unwrap_or(0);
        let mut screen_map = Vec::new();
        let mut rendered_content = std::collections::HashMap::new();
        let mut display_lines = Vec::new();
        let mut line_fill_styles = Vec::new();

        for idx in start..end {
            let Some(line) = document_lines.get(idx) else {
                continue;
            };
            let mut plain = line.plain.clone();
            if idx == start && plain.contains('│') {
                plain = normalize_rendered_table_plain(&plain);
            }
            let char_end = CharIdx::new(plain.chars().count());
            screen_map.push((idx, CharIdx::ZERO, char_end));
            rendered_content.insert(idx, plain);
            let spans = apply_selection_overlay(line, sel_range_for_line(view, line, idx));
            line_fill_styles.push(line.fill_style);
            display_lines.push(Line::from(spans).style(line.style));
        }

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        let before_status_lines = display_lines.len();
        self.append_status_lines(&mut display_lines, &spinner_line, live_status, view);
        let after_status_lines = display_lines.len();
        line_fill_styles.resize(display_lines.len(), None);
        let line_fill_styles = trim_line_fill_styles(line_fill_styles, area.height as usize);
        let display_lines = self.trim_to_area_height(display_lines, area.height as usize);
        crate::tui::log_trace!(
            "tui.output.render area={}x{} content={}x{} total_lines={} visible_lines={} auto_scroll={} scroll_offset={} range={}..{} needs_scrollbar={} spinner={} queued_lines={} task_lines={} before_status_lines={} after_status_lines={} after_trim_lines={} visible_overwide_lines={} max_visible_line_width={} first_visible={:?} last_visible={:?} last_doc={:?}",
            area.width,
            area.height,
            content_area.width,
            content_area.height,
            total_lines,
            visible_lines,
            view.auto_scroll,
            view.scroll_offset,
            start,
            end,
            needs_scrollbar,
            spinner_line.is_some(),
            live_status.queued_lines.len(),
            live_status.task_lines.len(),
            before_status_lines,
            after_status_lines,
            display_lines.len(),
            visible_overwide_lines,
            max_visible_line_width,
            first_visible_doc_line_plain,
            last_visible_doc_line_plain,
            last_doc_line_plain
        );

        // 根因修复（#196）：paragraph.render 复用 ratatui buffer cell 数组，
        // 上一帧长行 → 本帧短行时尾部 cell 不会被清，导致"行重叠 / 残影"。
        // 在 paint 背景与 paragraph 渲染之前先把 content_area 整片 reset。
        clear_area(content_area, buf);
        paint_line_fill_styles(content_area, buf, &line_fill_styles);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(display_lines);
            paragraph.render(content_area, buf);
        }));

        render_scrollbar(
            area,
            buf,
            total_lines,
            visible_lines,
            view.auto_scroll,
            view.scroll_offset,
        );
    }
}

fn sel_range_for_line(
    view: &OutputViewState,
    line: &crate::tui::render::output::rendered::RenderedLine,
    line_idx: usize,
) -> Option<SelRange> {
    let (start, end) = view.selection_range()?;
    sel_range_for_bounds(start, end, line_idx, line.plain.chars().count())
}

pub(crate) fn sel_range_for_bounds(
    start: SelectionAnchor,
    end: SelectionAnchor,
    line_idx: usize,
    plain_len: usize,
) -> Option<SelRange> {
    let (start_line, start_col) = start;
    let (end_line, end_col) = end;
    if line_idx < start_line || line_idx > end_line {
        return None;
    }
    let start = if line_idx == start_line {
        start_col.as_usize().min(plain_len)
    } else {
        0
    };
    let end = if line_idx == end_line {
        end_col.as_usize().min(plain_len)
    } else {
        plain_len
    };
    (start < end).then_some(SelRange { start, end })
}

fn diagnostic_plain(value: &str) -> String {
    const MAX_CHARS: usize = 96;
    let mut out = value
        .chars()
        .take(MAX_CHARS)
        .collect::<String>()
        .replace('\n', "\\n");
    if value.chars().count() > MAX_CHARS {
        out.push('…');
    }
    out
}

fn normalize_rendered_table_plain(plain: &str) -> String {
    let Some((left, right)) = plain.split_once('│') else {
        return plain.to_string();
    };
    format!("{}  │{}", left.trim_end(), right.trim_end())
}

pub(crate) fn content_area_for_scrollbar(area: Rect, needs_scrollbar: bool) -> Rect {
    if !needs_scrollbar || area.width == 0 {
        return area;
    }
    Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(SCROLLBAR_RESERVE_COLS).max(1),
        height: area.height,
    }
}

fn visible_range(
    total_lines: usize,
    visible_lines: usize,
    auto_scroll: bool,
    scroll_offset: usize,
) -> (usize, usize) {
    if auto_scroll {
        let start = total_lines.saturating_sub(visible_lines);
        (start, total_lines)
    } else {
        let max_start = total_lines.saturating_sub(visible_lines);
        let start = max_start.saturating_sub(scroll_offset).min(max_start);
        (start, (start + visible_lines).min(total_lines))
    }
}

fn clear_area(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
        }
    }
}

fn paint_line_fill_styles(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    fill_styles: &[Option<Style>],
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    for (row, fill_style) in fill_styles.iter().enumerate() {
        if row >= area.height as usize {
            break;
        }
        let Some(style) = fill_style else {
            continue;
        };
        let y = area.y + row as u16;
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                // 最小修复（#196）：set_style 前先 reset 清空旧 symbol，
                // 作为 paragraph.render 前 clear_area 之后的双保险。
                // Cell::reset 签名是 &mut self，不能链式调用。
                cell.reset();
                cell.set_style(*style);
            }
        }
    }
}

fn trim_line_fill_styles(styles: Vec<Option<Style>>, height: usize) -> Vec<Option<Style>> {
    let len = styles.len();
    if len > height {
        styles.into_iter().skip(len - height).collect()
    } else {
        styles
    }
}

fn render_scrollbar(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    total_lines: usize,
    visible_lines: usize,
    auto_scroll: bool,
    scroll_offset: usize,
) {
    if total_lines <= visible_lines {
        return;
    }
    let scrollbar_area = Rect {
        x: area.right().saturating_sub(1),
        y: area.top(),
        width: 1,
        height: area.height,
    };
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let current_position = if auto_scroll {
        max_scroll
    } else {
        max_scroll.saturating_sub(scroll_offset)
    };
    let mut scrollbar_state = ScrollbarState::new(max_scroll).position(current_position);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    StatefulWidget::render(scrollbar, scrollbar_area, buf, &mut scrollbar_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
    use crate::tui::render::output_area::selection::output_selection_view_for_test;
    use crate::tui::render::theme;
    use crate::tui::view_model::LiveStatusViewModel;
    use ratatui::{buffer::Buffer, layout::Rect, text::Span};
    use sdk::CharIdx;

    fn no_live_status() -> LiveStatusViewModel {
        LiveStatusViewModel::default()
    }

    #[test]
    fn test_render_reserves_scrollbar_column_and_wraps_long_lines() {
        let mut area = OutputArea::new();
        area.replace_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![
                    RenderedLine::new(vec![Span::raw("line 1")]),
                    RenderedLine::new(vec![Span::raw("1234567")]),
                    RenderedLine::new(vec![Span::raw("line 2")]),
                ],
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
                lines: vec![RenderedLine::new(vec![Span::raw("hello")])],
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
                lines: vec![
                    RenderedLine::from_plain("hi").with_fill_style(fill),
                    RenderedLine::empty().with_fill_style(fill),
                ],
            }],
        });
        let area_rect = Rect::new(0, 0, 8, 3);
        let view = OutputViewState {
            last_visible_height: 3,
            ..Default::default()
        };
        let mut buf = Buffer::empty(area_rect);

        area.render(area_rect, &mut buf, &view, &no_live_status());

        for x in 0..8 {
            assert_eq!(buf[(x, 0)].bg, ratatui::style::Color::Blue);
            assert_eq!(buf[(x, 1)].bg, ratatui::style::Color::Blue);
        }
        assert_ne!(buf[(0, 2)].bg, ratatui::style::Color::Blue);
    }

    #[test]
    fn test_output_area_selection_overrides_fill_style_on_text_cells() {
        let fill = ratatui::style::Style::default().bg(ratatui::style::Color::Blue);
        let mut area = OutputArea::new();
        area.replace_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "filled".into(),
                lines: vec![RenderedLine::from_plain("hello").with_fill_style(fill)],
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
        assert_eq!(buf[(7, 0)].bg, ratatui::style::Color::Blue);
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
                lines: vec![line],
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
                lines: vec![line],
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
            buf[(10, 0)].bg,
            theme::USER_BG,
            "行尾空白也应有用户消息背景"
        );
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
                lines: vec![line],
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

    /// 回归 #196：ratatui 0.29 `Paragraph::render` 复用 buffer cell 数组，
    /// 上一帧长行 → 本帧短行时尾部 cell 不会被覆盖。修复在 `paragraph.render`
    /// 之前对 `content_area` 调一次 `clear_area`，确保行变短时不残留旧 symbol。
    #[test]
    fn test_render_clears_content_area_between_frames_preventing_cell_leak() {
        let mut area = OutputArea::new();
        let area_rect = Rect::new(0, 0, 10, 1);
        let view = OutputViewState {
            last_visible_height: 1,
            ..Default::default()
        };
        let mut buf = Buffer::empty(area_rect);

        // 第一帧：长行 "abcdefghij" 占满 10 列。
        area.replace_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "first".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("abcdefghij")])],
            }],
        });
        area.render(area_rect, &mut buf, &view, &no_live_status());

        // 模拟帧间 buffer 复用：把第 0 行所有 cell 涂上易识别的 'L' 残留，
        // 避免"恰好相同"误判。
        for x in 0..10 {
            buf[(x, 0)].set_symbol("L");
        }

        // 第二帧：短行 "xy" 只占 2 列。
        area.replace_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "second".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("xy")])],
            }],
        });
        area.render(area_rect, &mut buf, &view, &no_live_status());

        assert_eq!(buf[(0, 0)].symbol(), "x");
        assert_eq!(buf[(1, 0)].symbol(), "y");
        // 关键断言：第 2..10 列必须被清空，不再保留上一帧的 'L'。
        for x in 2..10 {
            assert_eq!(
                buf[(x, 0)].symbol(),
                " ",
                "列 {x} 残留了上一帧的 'L'（cell-leak regression #196）"
            );
        }
    }
}
