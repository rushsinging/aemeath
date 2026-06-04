use ratatui::{style::Style, text::Line};

use sdk::CharIdx;

use crate::tui::render::display::safe_text::clamp_split_index;
use crate::tui::render::theme;

use crate::tui::render::output::selection_overlay::{apply_selection_overlay_with_fg, SelRange};
use crate::tui::render::output_area::render::sel_range_for_bounds;
use crate::tui::render::output_area::OutputArea;
use crate::tui::view_state::output::OutputViewState;

impl OutputArea {
    pub(crate) fn append_status_lines(
        &mut self,
        lines: &mut Vec<Line<'static>>,
        spinner_line: &Option<Line<'static>>,
        queued_lines: &[String],
        task_status_lines: &[String],
        view: &OutputViewState,
    ) {
        // 排队输入预览行（固定在 spinner 上方）
        if !queued_lines.is_empty() {
            let base_idx = self.document.total_lines();
            for (i, text) in queued_lines.iter().enumerate() {
                let char_count = text.chars().count();
                self.screen_line_map
                    .push((base_idx + i, CharIdx::ZERO, CharIdx::new(char_count)));
                let style = Style::default().fg(theme::TEXT_DIM);
                let line = Line::from(apply_selection_overlay_with_fg(
                    &crate::tui::render::output::rendered::RenderedLine::new(vec![
                        ratatui::text::Span::styled(text.clone(), style),
                    ]),
                    selection_range_for_virtual_line(view, base_idx + i, char_count),
                    theme::SELECTION_FG,
                ));
                lines.push(line);
            }
        }
        if let Some(sl) = spinner_line {
            // spinner 行也加一个不可选的 screen_map entry
            self.screen_line_map
                .push((usize::MAX, CharIdx::ZERO, CharIdx::ZERO));
            lines.push(sl.clone());
        }
        if spinner_line.is_some() {
            let task_base_idx = self.document.total_lines();
            for (i, task_line) in task_status_lines.iter().enumerate() {
                let text = format!("  {task_line}");
                let char_count = text.chars().count();
                self.screen_line_map.push((
                    task_base_idx + i,
                    CharIdx::ZERO,
                    CharIdx::new(char_count),
                ));
                let task_style = task_status_style(task_line);
                let line = Line::from(apply_selection_overlay_with_fg(
                    &crate::tui::render::output::rendered::RenderedLine::new(vec![
                        ratatui::text::Span::styled(text, task_style),
                    ]),
                    selection_range_for_virtual_line(view, task_base_idx + i, char_count),
                    theme::SELECTION_FG,
                ));
                lines.push(line);
            }
        }
    }

    pub(crate) fn trim_to_area_height(
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
}

fn selection_range_for_virtual_line(
    view: &OutputViewState,
    line_idx: usize,
    plain_len: usize,
) -> Option<SelRange> {
    let (start, end) = view.selection_range()?;
    sel_range_for_bounds(start, end, line_idx, plain_len)
}

fn task_status_style(text: &str) -> Style {
    if text.starts_with('✓') || text.trim_start().starts_with('✓') {
        Style::default().fg(theme::SUCCESS)
    } else if text.starts_with('■') || text.trim_start().starts_with('■') {
        Style::default().fg(theme::TOOL_RUNNING)
    } else if text.starts_with('□') || text.trim_start().starts_with('□') {
        Style::default().fg(theme::TEXT_MUTED)
    } else if text.starts_with('…') || text.trim_start().starts_with('…') {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::BORDER)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::*;
    use crate::tui::render::output_area::selection::output_selection_view_for_test;
    use crate::tui::render::output_area::types::SpinnerState;

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
            phase_start: std::time::Instant::now(),
        });
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf, &Default::default());
        // screen_line_map: spinner(1,不可选) + task_status(2) = 3
        assert_eq!(output.screen_line_map.len(), 3);
        // spinner 在 index 0, 不可选(usize::MAX)
        assert_eq!(output.screen_line_map[0].0, usize::MAX);
        let base = output.document().total_lines();
        assert_eq!(output.screen_line_map[1].0, base);
        assert_eq!(output.screen_line_map[2].0, base + 1);
        assert_eq!(output.task_status_lines.len(), 2);
        // rel_row=2 对应第 2 个 task_status 行
        let s = output.screen_to_anchor(2, 0, &area).unwrap();
        let e = output.screen_to_anchor(2, 15, &area).unwrap();
        let view = output_selection_view_for_test(s, e);

        assert_eq!(
            output.selected_text_for_view(&view),
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
            phase_start: std::time::Instant::now(),
        });
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf, &Default::default());
        // screen_map: [spinner(usize::MAX), task_status(lines.len())]
        // 选 task_status 行（screen 行 1）
        let s = output.screen_to_anchor(1, 0, &area).unwrap();
        let e = output.screen_to_anchor(1, 8, &area).unwrap();
        let view = output_selection_view_for_test(s, e);
        output.render(area, &mut buf, &view);

        let first_selected = buf.cell((area.x, area.y + 1)).unwrap();
        assert_eq!(first_selected.style().bg, Some(theme::SELECTION_BG));
        assert_eq!(first_selected.style().fg, Some(theme::SELECTION_FG));

        let unselected = buf.cell((area.x + 9, area.y + 1)).unwrap();
        assert_ne!(unselected.style().bg, Some(theme::SELECTION_BG));
    }
}
