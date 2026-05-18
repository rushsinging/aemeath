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
        task_status_lines: &[String],
    ) {
        lines.extend(queued_lines);
        if let Some(sl) = spinner_line {
            lines.push(sl.clone());
        }
        if spinner_line.is_some() {
            let screen_start = self.screen_line_map.len();
            let task_base_idx = self.lines.len();
            for (i, task_line) in task_status_lines.iter().enumerate() {
                let text = format!("  {task_line}");
                let char_count = text.chars().count();
                self.screen_line_map.push((
                    task_base_idx + i,
                    CharIdx::ZERO,
                    CharIdx::new(char_count),
                ));
                let screen_idx = screen_start + i;
                let task_style = Style::default().fg(Color::DarkGray);
                let line = if self.has_real_selection() {
                    Line::from(self.render_line_with_selection(
                        screen_idx,
                        &text,
                        task_style,
                        &self.screen_line_map,
                    ))
                } else {
                    Line::styled(text, task_style)
                };
                lines.push(line);
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

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::*;
    use crate::tui::output_area::types::SpinnerState;

    #[test]
    fn test_render_maps_task_status_lines_for_selection() {
        let mut output = OutputArea::new();
        output.task_status_lines =
            vec!["━━ Tasks: 0/1 ━━".to_string(), "□ #1 修复 bug".to_string()];
        output.spinner = Some(SpinnerState {
            frame: 0,
            verb: "Thinking".to_string(),
            start: std::time::Instant::now(),
            phase: None,
        });
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf);
        assert_eq!(output.screen_line_map.len(), 2);
        assert_eq!(output.screen_line_map[1].0, output.lines.len() + 1);
        assert_eq!(output.task_status_lines.len(), 2);
        output.start_selection(1, 0, &area);
        output.update_selection(1, 15, &area);

        assert_eq!(
            output.get_selected_text(),
            Some("  □ #1 修复 bug".to_string())
        );
    }

    #[test]
    fn test_render_highlights_selected_task_status_line() {
        let mut output = OutputArea::new();
        output.task_status_lines = vec!["□ #1 修复 bug".to_string()];
        output.spinner = Some(SpinnerState {
            frame: 0,
            verb: "Thinking".to_string(),
            start: std::time::Instant::now(),
            phase: None,
        });
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf);
        output.start_selection(0, 0, &area);
        output.update_selection(0, 8, &area);
        output.render(area, &mut buf);

        let first_selected = buf.cell((area.x, area.y + 1)).unwrap();
        assert_eq!(first_selected.style().bg, Some(ratatui::style::Color::Blue));
        assert_eq!(
            first_selected.style().fg,
            Some(ratatui::style::Color::White)
        );

        let unselected = buf.cell((area.x + 9, area.y + 1)).unwrap();
        assert_ne!(unselected.style().bg, Some(ratatui::style::Color::Blue));
    }
}
