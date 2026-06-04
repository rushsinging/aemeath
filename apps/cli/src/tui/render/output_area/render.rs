use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use sdk::CharIdx;

use super::OutputArea;
use crate::tui::render::output::selection_overlay::{apply_selection_overlay, SelRange};
use crate::tui::view_state::output::{OutputViewState, SelectionAnchor};

impl OutputArea {
    /// 渲染输出区域
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        view: &OutputViewState,
    ) {
        if area.height == 0 {
            return;
        }

        let new_width = (area.width as usize).saturating_sub(2);
        if new_width != self.term_width {
            self.term_width = new_width;
        }

        let spinner_line = self.build_spinner_line();
        let task_line_count = if self.spinner.is_some() {
            self.task_status_lines.len()
        } else {
            0
        };
        let queued_line_count = self.queued_submission_lines.len();
        let reserved = if spinner_line.is_some() {
            queued_line_count + 1 + task_line_count
        } else if queued_line_count > 0 {
            queued_line_count
        } else {
            0
        };

        let visible_lines = (area.height as usize).saturating_sub(reserved);
        self.last_visible_height = visible_lines;
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
            display_lines.push(Line::from(spans));
        }

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        let queued_lines = self.queued_submission_lines.clone();
        let task_status_lines = self.task_status_lines.clone();
        self.append_status_lines(
            &mut display_lines,
            &spinner_line,
            &queued_lines,
            &task_status_lines,
            view,
        );
        let display_lines = self.trim_to_area_height(display_lines, area.height as usize);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(display_lines);
            paragraph.render(content_area, buf);
        }));

        let total_rendered = self.screen_line_map.len();
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
        self.last_line_count = total_lines;
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
    use ratatui::{buffer::Buffer, layout::Rect, text::Span};
    use sdk::CharIdx;

    #[test]
    fn test_render_reserves_scrollbar_column_and_wraps_long_lines() {
        let mut area = OutputArea::new();
        area.set_document(RenderedDocument {
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
        let mut buf = Buffer::empty(area_rect);

        area.render(area_rect, &mut buf, &Default::default());

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
        area.set_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("hello")])],
            }],
        });
        let view = output_selection_view_for_test((0, CharIdx::new(0)), (0, CharIdx::new(3)));
        let area_rect = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area_rect);
        area.render(area_rect, &mut buf, &view);

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
        area.set_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![line],
            }],
        });
        // 选中 plain 字符 [0,3) = "hel"
        let view = output_selection_view_for_test((0, CharIdx::new(0)), (0, CharIdx::new(3)));
        let area_rect = Rect::new(0, 0, 12, 3);
        let mut buf = Buffer::empty(area_rect);
        area.render(area_rect, &mut buf, &view);

        // gutter 占屏幕列 0..2，绝不高亮。
        assert_ne!(buf[(0, 0)].bg, theme::SELECTION_BG, "gutter 列不选中");
        assert_ne!(buf[(1, 0)].bg, theme::SELECTION_BG, "gutter 列不选中");
        // 内容从屏幕列 2 起，"hel" 高亮 → 列 2,3,4。
        assert_eq!(buf[(2, 0)].bg, theme::SELECTION_BG, "内容首字符 h 高亮");
        assert_eq!(buf[(4, 0)].bg, theme::SELECTION_BG, "内容第三字符 l 高亮");
        assert_ne!(buf[(5, 0)].bg, theme::SELECTION_BG, "第四字符 l 不在选区");
    }

    #[test]
    fn test_click_on_gutter_line_maps_to_content_char() {
        let mut line =
            RenderedLine::with_plain(vec![Span::raw("✓ "), Span::raw("hello")], "hello".into());
        line.gutter_cols = 2;
        let mut area = OutputArea::new();
        area.set_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![line],
            }],
        });
        let area_rect = Rect::new(0, 0, 12, 3);
        let mut buf = Buffer::empty(area_rect);
        area.render(area_rect, &mut buf, &Default::default());

        // 点击屏幕列 2（内容 "h"）→ plain 字符 0；拖到列 5（内容 "l" 之后）→ plain 3。
        // 经只读换算 screen_to_anchor 折算锚点后直接置选区镜像（widget start/update_selection
        // 已删除，选区真相迁至 view_state）。
        let s = area.screen_to_anchor(0, 2, &area_rect).unwrap();
        let e = area.screen_to_anchor(0, 5, &area_rect).unwrap();
        let view = output_selection_view_for_test(s, e);
        assert_eq!(area.selected_text_for_view(&view).as_deref(), Some("hel"));

        // 点击 gutter 区间（列 0）→ 映射到 plain 字符 0，不偏移。
        let s = area.screen_to_anchor(0, 0, &area_rect).unwrap();
        let e = area.screen_to_anchor(0, 4, &area_rect).unwrap();
        let view = output_selection_view_for_test(s, e);
        assert_eq!(
            area.selected_text_for_view(&view).as_deref(),
            Some("he"),
            "点击 gutter 钳到 plain 0，拖到列 4 选到内容字符 2"
        );
    }
}
