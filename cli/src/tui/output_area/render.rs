use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use super::OutputArea;
use super::types::OutputLine;
use super::display;

impl OutputArea {
    /// 渲染输出区域
    pub fn render(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }

        let new_width = (area.width as usize).saturating_sub(2);
        if new_width != self.term_width {
            self.term_width = new_width;
            self.rendered_cache.invalidate();
        }

        let spinner_line = self.build_spinner_line();
        let queued_lines = self.build_queued_message_lines();
        let task_line_count = if self.spinner.is_some() {
            self.task_status_lines.len()
        } else {
            0
        };
        let queued_count = queued_lines.len();
        let reserved = if spinner_line.is_some() {
            1 + task_line_count + queued_count
        } else {
            queued_count
        };

        let visible_lines = (area.height as usize).saturating_sub(reserved);
        self.last_visible_height = visible_lines;
        let total_lines = self.lines.len();
        let (start, end) = visible_range(
            total_lines,
            visible_lines,
            self.auto_scroll,
            self.scroll_offset,
        );

        clear_area(area, buf);
        let spinner_frame_idx = self.spinner.as_ref().map(|s| s.frame).unwrap_or(0);

        // 从渲染缓存获取渲染结果
        // VecDeque 不支持 &[T]，转为 Vec 传给缓存层
        let lines_vec: Vec<OutputLine> = self.lines.iter().cloned().collect();
        self.rendered_cache.ensure_rendered(
            &lines_vec,
            start,
            end,
            self.term_width,
        );

        // 构建显示行
        let mut screen_map = Vec::new();
        let mut rendered_content = std::collections::HashMap::new();
        let mut display_lines = Vec::new();

        for i in start..end {
            if let Some(ref rendered) = self.rendered_cache.get(i) {
                if let Some(text) = &rendered.rendered_text {
                    rendered_content.insert(i, text.clone());
                }
                screen_map.extend(rendered.screen_entries.clone());
                display_lines.push(rendered.line.clone());
            } else {
                // 未渲染的行，用空行占位
                display_lines.push(Line::raw(""));
            }
        }

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        let task_status_lines = self.task_status_lines.clone();
        self.append_status_lines(&mut display_lines, queued_lines, &spinner_line, &task_status_lines);
        let display_lines = self.trim_to_area_height(display_lines, area.height as usize);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(display_lines);
            paragraph.render(area, buf);
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

        self.color_tool_call_dots(area, buf, spinner_frame_idx, total_rendered);
        render_scrollbar(
            area,
            buf,
            total_lines,
            visible_lines,
            self.auto_scroll,
            self.scroll_offset,
        );
        self.last_line_count = total_lines;
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

/// 供 rendered_cache.rs 使用
pub fn wrap_line(content: &str, max_width: usize) -> Vec<String> {
    display::wrap_line(content, max_width)
}
