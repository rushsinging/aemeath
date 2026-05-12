use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
};

use aemeath_core::string_idx::CharIdx;

use crate::tui::safe_text::clamp_split_index;

use super::{LineStyle, OutputArea};

impl OutputArea {
    pub(super) fn append_status_lines(
        &mut self,
        lines: &mut Vec<Line<'static>>,
        queued_lines: Vec<Line<'static>>,
        spinner_line: &Option<Line<'static>>,
    ) {
        lines.extend(queued_lines);
        if let Some(sl) = spinner_line {
            lines.push(sl.clone());
        }
        if spinner_line.is_some() {
            let base_idx = self.lines.len();
            for (i, task_line) in self.task_status_lines.iter().enumerate() {
                let text = format!("  {task_line}");
                let char_count = text.chars().count();
                self.screen_line_map
                    .push((base_idx + i, CharIdx::ZERO, CharIdx::new(char_count)));
                lines.push(Line::styled(text, Style::default().fg(Color::DarkGray)));
            }
        }
    }

    pub(super) fn trim_to_area_height(
        &mut self,
        lines: Vec<Line<'static>>,
        height: usize,
    ) -> Vec<Line<'static>> {
        if lines.len() > height {
            let offset = lines.len() - height;
            log::debug!(
                "trim: lines.len={}, area.height={}, offset={}, screen_map.len={}",
                lines.len(),
                height,
                offset,
                self.screen_line_map.len()
            );
            let mapped_drop = clamp_split_index(offset, self.screen_line_map.len());
            self.screen_line_map = self.screen_line_map.split_off(mapped_drop);
            let visible_map_len = self.screen_line_map.len().min(height);
            self.screen_line_map.truncate(visible_map_len);
            lines.into_iter().skip(offset).collect()
        } else {
            let visible_map_len = self.screen_line_map.len().min(lines.len());
            self.screen_line_map.truncate(visible_map_len);
            lines
        }
    }

    pub(super) fn color_tool_call_dots(
        &self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        spinner_frame_idx: u64,
        total_rendered: usize,
    ) {
        let blink_on = (spinner_frame_idx / 10) % 2 == 0;
        for (si, &(li, _, _)) in self.screen_line_map.iter().enumerate() {
            if li >= self.lines.len() {
                continue;
            }
            let line = &self.lines[li];
            let dot_color = match line.style {
                LineStyle::ToolCallRunning if line.content.starts_with('●') => {
                    Some(if blink_on {
                        Color::White
                    } else {
                        Color::DarkGray
                    })
                }
                LineStyle::ToolCallSuccess if line.content.starts_with('✓') => Some(Color::Green),
                LineStyle::ToolCallError if line.content.starts_with('✗') => Some(Color::Red),
                _ => None,
            };
            if let Some(color) = dot_color {
                let visible_offset = total_rendered.saturating_sub(area.height as usize);
                let screen_y = si.saturating_sub(visible_offset);
                if screen_y >= area.height as usize {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((area.x, area.y + screen_y as u16)) {
                    cell.set_char('●');
                    let mut style = cell.style();
                    style.fg = Some(color);
                    cell.set_style(style);
                }
            }
        }
    }
}
