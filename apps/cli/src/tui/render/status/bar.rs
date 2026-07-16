#[path = "../display/status_bar_format.rs"]
mod status_bar_format;
#[path = "../display/status_bar_selection.rs"]
mod status_bar_selection;

use crate::tui::render::theme;
use crate::tui::view_model::{
    StatusNoticeViewKind, StatusRuntimeViewModel, StatusViewModel, StatusWorktreeKind,
};
use crate::tui::view_state::StatusSelectionViewState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
pub use status_bar_format::WorktreeKind;
use status_bar_format::{context_row_text, StatusLineContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusBarRow {
    Runtime,
    Context,
}

/// The status bar at the bottom of the screen.
///
/// Stateless widget shell: all runtime/diagnostic/status text comes from
/// `StatusViewModel` at render time. `StatusBar` only keeps static display
/// configuration that is not part of app/runtime state.
pub struct StatusBar {
    context: StatusLineContext,
}

#[derive(Clone, Copy)]
enum RuntimeSegmentStyle {
    Model,
    Border,
    Status(StatusNoticeViewKind),
    Muted,
    ContextPct(u64),
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            context: StatusLineContext::default(),
        }
    }

    pub fn draw(
        &self,
        area: Rect,
        buf: &mut Buffer,
        selection: &StatusSelectionViewState,
        view: &StatusViewModel,
    ) {
        self.render(area, buf, selection, view);
    }

    #[cfg(test)]
    pub(crate) fn set_permission_mode_for_test(&mut self, mode: impl Into<String>) {
        self.context.permission_mode = mode.into();
    }

    /// Set permission mode text for the status context.
    pub fn set_permission_mode(&mut self, mode: impl Into<String>) {
        self.context.permission_mode = mode.into();
    }

    fn context_for_view(&self, view: &StatusViewModel) -> StatusLineContext {
        let runtime = &view.runtime;
        let mut context = self.context.clone();
        context.path_base = runtime.context.path_base.clone();
        context.workspace_root = runtime.context.workspace_root.clone();
        context.worktree_kind = match runtime.context.kind {
            StatusWorktreeKind::Worktree => WorktreeKind::Worktree,
            StatusWorktreeKind::Main => WorktreeKind::Main,
        };
        context.branch = runtime.context.branch.clone();
        context.session_id = runtime.session_id.clone();
        context
    }

    pub(crate) fn context_row_text_for_view(&self, width: usize, view: &StatusViewModel) -> String {
        context_row_text(&self.context_for_view(view), width)
    }

    /// Render the status bar
    pub fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        selection: &StatusSelectionViewState,
        view: &StatusViewModel,
    ) {
        if area.height == 0 {
            return;
        }

        // 窄屏提示：终端宽度 < 40 时，状态栏只显示提示
        if area.width < crate::tui::render::output::gutter::NARROW_STATUS_HINT_THRESHOLD {
            let base = Style::default().bg(theme::STATUS_BG).fg(theme::WARNING);
            let hint = "[窄屏] 窗口过窄，建议调整终端大小";
            let spans = vec![Span::styled(hint, base)];
            for row in 0..area.height {
                let row_area = Rect {
                    y: area.y + row,
                    height: 1,
                    ..area
                };
                Paragraph::new(Line::from(spans.clone()))
                    .style(base)
                    .render(row_area, buf);
            }
            return;
        }

        let base = Style::default().bg(theme::STATUS_BG);
        let runtime_area = Rect { height: 1, ..area };
        // Default status selection points at Runtime, but an empty view_state short-circuits in
        // spans_with_selection(), so no highlight is applied unless a real range exists.
        let runtime_line = if selection.selection_row == StatusBarRow::Runtime {
            self.runtime_row_spans_with_selection(selection, base, view)
        } else {
            self.runtime_row_spans(view)
        };
        Paragraph::new(Line::from(runtime_line))
            .style(base)
            .render(runtime_area, buf);

        if area.height >= 2 {
            let context_area = Rect {
                y: area.y.saturating_add(1),
                height: 1,
                ..area
            };
            let context_line = self.context_row_spans(area.width as usize, base, selection, view);
            Paragraph::new(Line::from(context_line))
                .style(base)
                .render(context_area, buf);
        }
    }

    fn runtime_segments(&self, view: &StatusViewModel) -> Vec<(String, RuntimeSegmentStyle)> {
        let vm = &view.runtime;
        let mut segments = Vec::new();
        segments.push((
            format!(" {} ", view.notice.text),
            RuntimeSegmentStyle::Status(view.notice.kind),
        ));
        if let Some(ref model) = vm.model {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" {} ", model), RuntimeSegmentStyle::Model));
        }
        segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        segments.push((
            format!(" in {} ", sdk::format_tokens(vm.input_tokens)),
            RuntimeSegmentStyle::Muted,
        ));
        segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        segments.push((
            format!(" out {} ", sdk::format_tokens(vm.output_tokens)),
            RuntimeSegmentStyle::Muted,
        ));
        if vm.tps > 0.0 {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" {:.0} t/s ", vm.tps), RuntimeSegmentStyle::Muted));
        }
        if vm.context_size > 0 {
            let pct = self.context_pct(vm);
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((
                format!(" ctx {}% ", pct),
                RuntimeSegmentStyle::ContextPct(pct),
            ));
        }
        if vm.api_calls > 0 {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((
                format!(" api {} ", vm.api_calls),
                RuntimeSegmentStyle::Muted,
            ));
        }
        segments
    }

    fn context_pct(&self, vm: &StatusRuntimeViewModel) -> u64 {
        if vm.context_size > 0 && vm.last_input_tokens > 0 {
            vm.last_input_tokens * 100 / vm.context_size
        } else {
            0
        }
    }

    fn runtime_segment_style(&self, style: RuntimeSegmentStyle) -> Style {
        match style {
            RuntimeSegmentStyle::Model => Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
            RuntimeSegmentStyle::Border => Style::default().fg(theme::BORDER),
            RuntimeSegmentStyle::Status(StatusNoticeViewKind::Normal) => {
                Style::default().fg(theme::TEXT)
            }
            RuntimeSegmentStyle::Status(StatusNoticeViewKind::Success) => {
                Style::default().fg(theme::SUCCESS)
            }
            RuntimeSegmentStyle::Status(StatusNoticeViewKind::Warning) => {
                Style::default().fg(theme::WARNING)
            }
            RuntimeSegmentStyle::Muted => Style::default().fg(theme::TEXT_MUTED),
            RuntimeSegmentStyle::ContextPct(pct) if pct >= 80 => Style::default().fg(theme::ERROR),
            RuntimeSegmentStyle::ContextPct(pct) if pct >= 50 => {
                Style::default().fg(theme::WARNING)
            }
            RuntimeSegmentStyle::ContextPct(_) => Style::default().fg(theme::TEXT_MUTED),
        }
    }

    fn runtime_row_spans(&self, view: &StatusViewModel) -> Vec<Span<'static>> {
        self.runtime_segments(view)
            .into_iter()
            .map(|(text, style)| Span::styled(text, self.runtime_segment_style(style)))
            .collect()
    }

    fn context_row_spans(
        &self,
        width: usize,
        base: Style,
        selection: &StatusSelectionViewState,
        view: &StatusViewModel,
    ) -> Vec<Span<'static>> {
        let text = self.context_row_text_for_view(width, view);
        if selection.selection_row == StatusBarRow::Context {
            return self.spans_with_selection(text, base, selection);
        }
        let parts: Vec<&str> = text.split(" │ ").collect();
        let mut spans = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER)));
            }
            let style = if index == 0 {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if index == 1 {
                Style::default().fg(theme::SUCCESS)
            } else if index == 2 {
                Style::default().fg(theme::WARNING)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            spans.push(Span::styled((*part).to_string(), style));
        }
        spans
    }

    fn runtime_row_spans_with_selection(
        &self,
        selection: &StatusSelectionViewState,
        base: Style,
        view: &StatusViewModel,
    ) -> Vec<Span<'static>> {
        self.spans_with_selection(self.build_full_text(view), base, selection)
    }

    pub(crate) fn build_full_text(&self, view: &StatusViewModel) -> String {
        self.runtime_segments(view)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
#[path = "../display/status_bar_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "../display/status_bar_v2_tests.rs"]
mod v2_tests;
