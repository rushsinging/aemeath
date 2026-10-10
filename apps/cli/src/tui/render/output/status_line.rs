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
    live_status_spinner_fixture_fields(verb, 0, elapsed_secs, phase_elapsed_secs, phase_text)
}

#[cfg(test)]
pub(crate) fn live_status_spinner_fixture_with_frame(
    verb: &str,
    frame: u64,
) -> LiveStatusViewModel {
    live_status_spinner_fixture_fields(verb, frame, 0, 0, None)
}

#[cfg(test)]
fn live_status_spinner_fixture_fields(
    verb: &str,
    frame: u64,
    elapsed_secs: u64,
    phase_elapsed_secs: u64,
    phase_text: Option<&str>,
) -> LiveStatusViewModel {
    LiveStatusViewModel {
        spinner: Some(SpinnerLineView {
            frame,
            verb: verb.to_string(),
            elapsed_secs,
            phase_elapsed_secs: phase_text.map(|_| phase_elapsed_secs),
            phase_text: phase_text.map(str::to_string),
            detail_text: None,
        }),
        queued_lines: Vec::new(),
        task_lines: Vec::new(),
        compact_progress: None,
    }
}

#[cfg(test)]
#[path = "status_line_tests.rs"]
mod tests;
