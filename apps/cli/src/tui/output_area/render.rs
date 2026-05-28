use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use sdk::CharIdx;

use super::display;
use super::types::OutputLine;
use super::OutputArea;
use crate::tui::view_state::cache::ViewRenderCache;

impl OutputArea {
    /// 渲染输出区域
    pub fn render_with_cache(
        &mut self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        cache: &mut ViewRenderCache,
    ) {
        if area.height == 0 {
            return;
        }

        let new_width = (area.width as usize).saturating_sub(2);
        if new_width != self.term_width {
            self.term_width = new_width;
            cache.output.line_cache.invalidate();
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
        cache
            .output
            .line_cache
            .ensure_rendered(&lines_vec, start, end, self.term_width);

        // 构建显示行：按 \n 拆分 rendered.line，使 display_lines 与 screen_map 一一对应
        let mut screen_map = Vec::new();
        let mut rendered_content = std::collections::HashMap::new();
        let mut display_lines = Vec::new();
        let has_selection = self.has_real_selection();

        for i in start..end {
            if let Some(ref rendered) = cache.output.line_cache.get(i) {
                if let Some(text) = &rendered.rendered_text {
                    rendered_content.insert(i, text.clone());
                }
                screen_map.extend(rendered.screen_entries.clone());

                // 将 rendered.line 按 \n 拆分为子行
                let sub_lines = split_line_at_newlines(&rendered.line);
                for (sub_idx, sub_line) in sub_lines.into_iter().enumerate() {
                    let entry_idx = screen_map.len() - rendered.screen_entries.len() + sub_idx;
                    if has_selection && entry_idx < screen_map.len() {
                        display_lines.push(self.apply_selection_to_line(
                            entry_idx,
                            &sub_line,
                            &screen_map,
                        ));
                    } else {
                        display_lines.push(sub_line);
                    }
                }
            } else {
                // 未渲染的行，用空行占位，同时添加 screen_map entry 保持对齐
                screen_map.push((i, CharIdx::ZERO, CharIdx::ZERO));
                display_lines.push(Line::raw(""));
            }
        }

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        let task_status_lines = self.task_status_lines.clone();
        self.append_status_lines(
            &mut display_lines,
            queued_lines,
            &spinner_line,
            &task_status_lines,
        );
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

    /// Legacy render entry point. New code should pass `AppViewState.cache`
    /// through `render_with_cache` so render cache lives in view_state.
    #[allow(dead_code)]
    pub fn render(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let mut view_cache = ViewRenderCache::default();
        view_cache.output.line_cache = std::mem::take(&mut self.rendered_cache.line_cache);
        self.render_with_cache(area, buf, &mut view_cache);
        self.rendered_cache.line_cache = view_cache.output.line_cache;
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

/// 将含 \n 的 Line 拆分为不含 \n 的多个子 Line。
/// 每个 \n 分隔的片段成为独立的 Line，与 screen_entries 一一对应。
fn split_line_at_newlines(line: &Line<'static>) -> Vec<Line<'static>> {
    let has_newline = line.spans.iter().any(|s| s.content.contains('\n'));
    if !has_newline {
        return vec![line.clone()];
    }

    let mut result = Vec::new();
    let mut current_spans: Vec<ratatui::text::Span<'static>> = Vec::new();

    for span in &line.spans {
        if !span.content.contains('\n') {
            current_spans.push(span.clone());
            continue;
        }
        let parts: Vec<&str> = span.content.split('\n').collect();
        for (pi, part) in parts.into_iter().enumerate() {
            if pi > 0 {
                result.push(Line::from(std::mem::take(&mut current_spans)));
            }
            if !part.is_empty() {
                current_spans.push(ratatui::text::Span::styled(part.to_string(), span.style));
            }
        }
    }
    if !current_spans.is_empty() {
        result.push(Line::from(current_spans));
    }
    result
}
