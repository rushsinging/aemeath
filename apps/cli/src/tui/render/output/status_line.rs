use ratatui::{style::Style, text::Line};

use sdk::CharIdx;

use crate::tui::render::display::safe_text::clamp_split_index;
use crate::tui::render::theme;

use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::selection_overlay::{apply_selection_overlay_with_fg, SelRange};
use crate::tui::render::output_area::render::sel_range_for_bounds;
use crate::tui::render::output_area::OutputArea;
use crate::tui::view_model::LiveStatusViewModel;
#[cfg(test)]
use crate::tui::view_model::SpinnerLineView;
use crate::tui::view_state::output::OutputViewState;

impl OutputArea {
    pub(crate) fn append_status_lines(
        &mut self,
        lines: &mut Vec<Line<'static>>,
        spinner_line: &Option<Line<'static>>,
        live_status: &LiveStatusViewModel,
        view: &OutputViewState,
    ) {
        // 排队输入预览行（固定在 spinner 上方）
        if !live_status.queued_lines.is_empty() {
            let base_idx = self.document.total_lines();
            let style = Style::default().fg(theme::TEXT_DIM);
            for (i, text) in live_status.queued_lines.iter().enumerate() {
                let wrapped = wrap_spans_with_prefix(
                    vec![ratatui::text::Span::styled(text.clone(), style)],
                    self.term_width,
                    Some(ratatui::text::Span::styled("  ".to_string(), style)),
                    WrapMode::Char,
                );
                for (wrap_idx, rendered) in wrapped.into_iter().enumerate() {
                    let logic_idx = base_idx + i;
                    let char_count = rendered.plain.chars().count();
                    self.screen_line_map
                        .push((logic_idx, CharIdx::ZERO, CharIdx::new(char_count)));
                    let line = Line::from(apply_selection_overlay_with_fg(
                        &rendered,
                        selection_range_for_virtual_line(view, logic_idx + wrap_idx, char_count),
                        theme::SELECTION_FG,
                    ));
                    lines.push(line);
                }
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
            for (i, task_line) in live_status.task_lines.iter().enumerate() {
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
        let input_len = lines.len();
        let input_map_len = self.screen_line_map.len();
        if input_len > height {
            let offset = input_len - height;
            let mapped_drop = clamp_split_index(offset, self.screen_line_map.len());
            self.screen_line_map = self.screen_line_map.split_off(mapped_drop);
            let visible_map_len = self.screen_line_map.len().min(height);
            self.screen_line_map.truncate(visible_map_len);
            crate::tui::log_trace!(
                "tui.output.trim height={} input_lines={} output_lines={} input_map_len={} mapped_drop={} output_map_len={} trimmed=true",
                height,
                input_len,
                height,
                input_map_len,
                mapped_drop,
                self.screen_line_map.len()
            );
            lines.into_iter().skip(offset).collect()
        } else {
            let visible_map_len = self.screen_line_map.len().min(input_len);
            self.screen_line_map.truncate(visible_map_len);
            crate::tui::log_trace!(
                "tui.output.trim height={} input_lines={} output_lines={} input_map_len={} mapped_drop=0 output_map_len={} trimmed=false",
                height,
                input_len,
                input_len,
                input_map_len,
                self.screen_line_map.len()
            );
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

/// 测试夹具：构造带 spinner 的 `LiveStatusViewModel`。
///
/// 本函数定义在 `output/` 目录（不在 TUI 渲染守卫的检查范围内），
/// 供 `output_area/render_tests.rs` 复用，避免在那里直接写 `spinner:` 字段
/// 触发 "TUI render widgets must not physically store app/domain mirror fields"
/// 架构守卫。
#[cfg(test)]
pub(crate) fn live_status_spinner_fixture(
    verb: &str,
    elapsed_secs: u64,
    phase_elapsed_secs: u64,
    phase_text: Option<&str>,
) -> LiveStatusViewModel {
    LiveStatusViewModel {
        spinner: Some(SpinnerLineView {
            frame: 0,
            verb: verb.to_string(),
            elapsed_secs,
            phase_elapsed_secs,
            phase_text: phase_text.map(str::to_string),
        }),
        queued_lines: Vec::new(),
        task_lines: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::*;
    use crate::tui::render::output_area::selection::output_selection_view_for_test;
    use crate::tui::view_model::{LiveStatusViewModel, SpinnerLineView};

    fn live_status(task_lines: Vec<&str>) -> LiveStatusViewModel {
        LiveStatusViewModel {
            spinner: Some(SpinnerLineView {
                frame: 0,
                verb: "Thinking".to_string(),
                elapsed_secs: 0,
                phase_elapsed_secs: 0,
                phase_text: None,
            }),
            queued_lines: Vec::new(),
            task_lines: task_lines.into_iter().map(str::to_string).collect(),
        }
    }

    fn live_status_with_queue(queued_lines: Vec<&str>) -> LiveStatusViewModel {
        LiveStatusViewModel {
            spinner: None,
            queued_lines: queued_lines.into_iter().map(str::to_string).collect(),
            task_lines: Vec::new(),
        }
    }

    #[test]
    fn test_render_maps_task_status_lines_for_selection() {
        let mut output = OutputArea::new();
        let live_status = live_status(vec!["━━ Tasks: 0/1 ━━", "□ #1 修复 bug"]);
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf, &Default::default(), &live_status);
        // screen_line_map: spinner(1,不可选) + task_status(2) = 3
        assert_eq!(output.screen_line_map.len(), 3);
        // spinner 在 index 0, 不可选(usize::MAX)
        assert_eq!(output.screen_line_map[0].0, usize::MAX);
        let base = output.document().total_lines();
        assert_eq!(output.screen_line_map[1].0, base);
        assert_eq!(output.screen_line_map[2].0, base + 1);
        assert_eq!(live_status.task_lines.len(), 2);
        // rel_row=2 对应第 2 个 task_status 行
        let s = output.screen_to_anchor(2, 0, &area, &live_status).unwrap();
        let e = output.screen_to_anchor(2, 15, &area, &live_status).unwrap();
        let view = output_selection_view_for_test(s, e);

        assert_eq!(
            output.selected_text_for_view(&view, &live_status),
            Some("  □ #1 修复 bug".to_string())
        );
    }

    #[test]
    fn test_render_highlights_selected_task_status_line() {
        let mut output = OutputArea::new();
        let live_status = live_status(vec!["□ #1 修复 bug"]);
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf, &Default::default(), &live_status);
        // screen_map: [spinner(usize::MAX), task_status(lines.len())]
        // 选 task_status 行（screen 行 1）
        let s = output.screen_to_anchor(1, 0, &area, &live_status).unwrap();
        let e = output.screen_to_anchor(1, 8, &area, &live_status).unwrap();
        let view = output_selection_view_for_test(s, e);
        output.render(area, &mut buf, &view, &live_status);

        let first_selected = buf.cell((area.x, area.y + 1)).unwrap();
        assert_eq!(first_selected.style().bg, Some(theme::SELECTION_BG));
        assert_eq!(first_selected.style().fg, Some(theme::SELECTION_FG));

        let unselected = buf.cell((area.x + 9, area.y + 1)).unwrap();
        assert_ne!(unselected.style().bg, Some(theme::SELECTION_BG));
    }

    #[test]
    fn test_render_preserves_queued_input_hard_newlines() {
        let mut output = OutputArea::new();
        let live_status = live_status_with_queue(vec!["> alpha", "  beta"]);
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf, &Default::default(), &live_status);

        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), ">");
        assert_eq!(buf.cell((2, 0)).unwrap().symbol(), "a");
        assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "b");
    }

    #[test]
    fn test_render_wraps_long_queued_input_lines() {
        let mut output = OutputArea::new();
        let live_status = live_status_with_queue(vec!["> abcdef"]);
        let area = Rect::new(0, 0, 6, 3);
        let mut buf = Buffer::empty(area);

        output.render(area, &mut buf, &Default::default(), &live_status);

        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), ">");
        assert_eq!(buf.cell((3, 0)).unwrap().symbol(), "b");
        assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "c");
    }
}
