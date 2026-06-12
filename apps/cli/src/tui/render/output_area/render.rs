use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use sdk::CharIdx;

use super::OutputArea;
use crate::tui::render::output::selection_overlay::{apply_selection_overlay, SelRange};
use crate::tui::render::theme;
use crate::tui::view_model::LiveStatusViewModel;
use crate::tui::view_state::output::{OutputViewState, SelectionAnchor};

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
        let mut screen_map = Vec::new();
        let mut rendered_content = std::collections::HashMap::new();
        let mut display_lines = Vec::new();
        let mut user_line_backgrounds = Vec::new();

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
            user_line_backgrounds.push(is_user_message_line(line));
            display_lines.push(Line::from(spans));
        }

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        self.append_status_lines(&mut display_lines, &spinner_line, live_status, view);
        user_line_backgrounds.resize(display_lines.len(), false);
        let user_line_backgrounds = trim_line_flags(user_line_backgrounds, area.height as usize);
        let display_lines = self.trim_to_area_height(display_lines, area.height as usize);
        let display_line_count = display_lines.len();

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(display_lines);
            paragraph.render(content_area, buf);
        }));
        paint_user_message_line_background(content_area, buf, &user_line_backgrounds);

        let total_rendered = self.screen_line_map.len();
        log::debug!(
            target: "cli::tui::tool_flow",
            "output_area render area={}x{} doc_lines={} visible_lines={} range={}..{} display_lines={} spinner={} task_lines={} queued_lines={} auto_scroll={} scroll_offset={}",
            area.width,
            area.height,
            total_lines,
            visible_lines,
            start,
            end,
            display_line_count,
            spinner_line.is_some(),
            live_status.task_lines.len(),
            live_status.queued_lines.len(),
            view.auto_scroll,
            view.scroll_offset,
        );
        if total_rendered > 0 {
            log::debug!(
                "render: screen_map after trim: first=[{:?}], last=[{:?}], total={}",
                self.screen_line_map.first(),
                self.screen_line_map.last(),
                total_rendered,
            );
        }
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

fn normalize_rendered_table_plain(plain: &str) -> String {
    let Some((left, right)) = plain.split_once('│') else {
        return plain.to_string();
    };
    format!("{}  │{}", left.trim_end(), right.trim_end())
}

fn content_area_for_scrollbar(area: Rect, needs_scrollbar: bool) -> Rect {
    if !needs_scrollbar || area.width == 0 {
        return area;
    }
    Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(1),
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

fn paint_user_message_line_background(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    user_line_backgrounds: &[bool],
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    for (row, should_paint) in user_line_backgrounds.iter().enumerate() {
        if row >= area.height as usize {
            break;
        }
        if !should_paint {
            continue;
        }
        let y = area.y + row as u16;
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(theme::USER_BG);
            }
        }
    }
}

fn trim_line_flags(flags: Vec<bool>, height: usize) -> Vec<bool> {
    let len = flags.len();
    if len > height {
        flags.into_iter().skip(len - height).collect()
    } else {
        flags
    }
}

fn is_user_message_line(line: &crate::tui::render::output::rendered::RenderedLine) -> bool {
    line.spans
        .iter()
        .any(|span| span.style.bg == Some(theme::USER_BG))
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
        );
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
}
