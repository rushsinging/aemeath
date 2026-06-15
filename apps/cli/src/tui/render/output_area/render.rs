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
        width: area.width.saturating_sub(5).max(1),
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
