use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

use aemeath_core::string_idx::CharIdx;

use crate::tui::output_area::display::wrap_line;
use crate::tui::theme;

use super::{display, markdown, render_blocks, LineStyle, OutputArea, OutputLine};

impl OutputArea {
    /// 渲染输出区域
    pub fn render(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }

        if let Some(ref mut s) = self.spinner {
            s.frame = s.frame.wrapping_add(1);
        }
        self.term_width = (area.width as usize).saturating_sub(2);

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

        let vis_lines: Vec<(usize, &OutputLine)> = self
            .lines
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .collect();
        let code_info = render_blocks::scan_code_blocks(vis_lines.iter().copied());
        let table_block_lines = render_blocks::scan_table_blocks(&vis_lines);
        let table_render_cache = render_blocks::render_table_cache(&vis_lines, &table_block_lines);

        let mut screen_map = Vec::new();
        let mut rendered_content = std::collections::HashMap::new();
        let mut lines = self.render_visible_lines(
            &vis_lines,
            &code_info,
            &table_render_cache,
            &mut screen_map,
            &mut rendered_content,
        );

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        let task_status_lines = self.task_status_lines.clone();
        self.append_status_lines(&mut lines, queued_lines, &spinner_line, &task_status_lines);
        let lines = self.trim_to_area_height(lines, area.height as usize);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(lines);
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

    fn render_visible_lines(
        &self,
        vis_lines: &[(usize, &OutputLine)],
        code_info: &render_blocks::CodeBlockInfo,
        table_render_cache: &std::collections::HashMap<usize, Vec<Vec<Span<'static>>>>,
        screen_map: &mut Vec<(usize, CharIdx, CharIdx)>,
        rendered_content: &mut std::collections::HashMap<usize, String>,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut vi = 0;
        let code_style = Style::default().fg(theme::CODE);

        while vi < vis_lines.len() {
            let (idx, output_line) = vis_lines[vi];
            if let Some(table_rows) = table_render_cache.get(&idx) {
                self.render_table_rows(
                    idx,
                    output_line.style,
                    table_rows,
                    screen_map,
                    rendered_content,
                    &mut lines,
                );
                vi += table_rows.len();
                continue;
            }

            if code_info.code_fence_lines.contains(&idx) {
                self.render_code_fence(idx, code_info, screen_map, &mut lines);
                vi += 1;
                continue;
            }

            self.render_output_line(
                idx,
                output_line,
                code_info.code_block_lines.contains(&idx),
                code_style,
                screen_map,
                rendered_content,
                &mut lines,
            );
            vi += 1;
        }

        lines
    }

    fn render_table_rows(
        &self,
        idx: usize,
        style: LineStyle,
        table_rows: &[Vec<Span<'static>>],
        screen_map: &mut Vec<(usize, CharIdx, CharIdx)>,
        rendered_content: &mut std::collections::HashMap<usize, String>,
        lines: &mut Vec<Line<'static>>,
    ) {
        for (row_offset, row_spans) in table_rows.iter().enumerate() {
            let logic_idx = idx + row_offset;
            let line_text: String = row_spans
                .iter()
                .map(|s| s.content.clone().into_owned())
                .collect();
            rendered_content.insert(logic_idx, line_text.clone());
            let wrapped = self.push_wrapped_offsets(logic_idx, &line_text, screen_map);
            if self.has_real_selection() {
                let screen_start = screen_map.len() - wrapped.len();
                for (chunk_idx, chunk) in wrapped.into_iter().enumerate() {
                    let spans = self.render_line_with_selection(
                        screen_start + chunk_idx,
                        &chunk,
                        style.to_style(),
                        screen_map,
                    );
                    lines.push(Line::from(spans));
                }
            } else if wrapped.len() == 1 {
                lines.push(Line::from(row_spans.clone()));
            } else {
                lines.extend(
                    wrapped
                        .into_iter()
                        .map(|chunk| Line::styled(chunk, style.to_style())),
                );
            }
        }
    }

    fn render_output_line(
        &self,
        idx: usize,
        output_line: &OutputLine,
        is_code_block: bool,
        code_style: Style,
        screen_map: &mut Vec<(usize, CharIdx, CharIdx)>,
        rendered_content: &mut std::collections::HashMap<usize, String>,
        lines: &mut Vec<Line<'static>>,
    ) {
        let style = output_line.style;
        let is_markdown = matches!(
            style,
            LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
        );
        let rendered_plain = if is_markdown && !is_code_block {
            markdown::strip_inline_formatting(&output_line.content)
        } else {
            output_line.content.clone()
        };
        if rendered_plain != output_line.content {
            rendered_content.insert(idx, rendered_plain.clone());
        }
        let wrapped = self.push_wrapped_offsets(idx, &rendered_plain, screen_map);

        if self.has_real_selection() {
            let screen_start = screen_map.len() - wrapped.len();
            for (chunk_idx, chunk) in wrapped.into_iter().enumerate() {
                let screen_idx = screen_start + chunk_idx;
                let line = if is_code_block {
                    Line::from(
                        self.render_line_with_selection(screen_idx, &chunk, code_style, screen_map),
                    )
                } else {
                    Line::from(self.render_line_with_selection(
                        screen_idx,
                        &chunk,
                        style.to_style(),
                        screen_map,
                    ))
                };
                lines.push(line);
            }
        } else if is_code_block {
            lines.extend(
                wrapped
                    .into_iter()
                    .map(|chunk| Line::styled(chunk, code_style)),
            );
        } else if is_markdown {
            lines.extend(markdown::inline_markdown_lines(
                &output_line.content,
                style.to_style(),
                self.term_width,
            ));
        } else {
            lines.extend(
                wrapped
                    .into_iter()
                    .map(|chunk| Line::styled(chunk, style.to_style())),
            );
        }
    }

    fn render_code_fence(
        &self,
        idx: usize,
        code_info: &render_blocks::CodeBlockInfo,
        screen_map: &mut Vec<(usize, CharIdx, CharIdx)>,
        lines: &mut Vec<Line<'static>>,
    ) {
        let border_style = Style::default().fg(theme::BORDER);
        let label = if let Some(lang) = code_info.code_lang_label.get(&idx) {
            if lang.is_empty() {
                "─".repeat(self.term_width.max(1))
            } else {
                format!("── {} ", lang)
            }
        } else {
            "─".repeat(self.term_width.max(1))
        };
        let label = display::truncate_unicode_width(&label, self.term_width);
        screen_map.push((idx, CharIdx::ZERO, CharIdx::new(label.chars().count())));
        lines.push(Line::styled(label, border_style));
    }

    fn push_wrapped_offsets(
        &self,
        idx: usize,
        content: &str,
        screen_map: &mut Vec<(usize, CharIdx, CharIdx)>,
    ) -> Vec<String> {
        let sanitized = display::sanitize_for_display(content);
        let char_offsets = display::compute_char_offsets(&sanitized, self.term_width);
        let wrapped = wrap_line(content, self.term_width);
        for (chunk_idx, _) in wrapped.iter().enumerate() {
            let (char_start, char_end) = char_offsets
                .get(chunk_idx)
                .copied()
                .unwrap_or((CharIdx::ZERO, CharIdx::ZERO));
            screen_map.push((idx, char_start, char_end));
        }
        wrapped
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
