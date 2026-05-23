#[path = "status_bar_format.rs"]
mod status_bar_format;
#[path = "status_bar_selection.rs"]
mod status_bar_selection;

use crate::tui::theme;
use aemeath_core::cost::format_tokens;
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

/// The status bar at the bottom of the screen
pub struct StatusBar {
    status: String,
    status_type: StatusType,
    input_tokens: u64,
    output_tokens: u64,
    last_input_tokens: u64,
    session_id: Option<String>,
    api_calls: u64,
    model: Option<String>,
    context_size: u64,
    tps: f64,
    is_selecting: bool,
    selection_start: Option<usize>,
    selection_end: Option<usize>,
    selection_row: StatusBarRow,
    selection_width: u16,
    context: StatusLineContext,
    thinking: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum StatusType {
    #[default]
    Normal,
    Success,
    Warning,
}

#[derive(Clone, Copy)]
enum RuntimeSegmentStyle {
    Model,
    Border,
    Thinking(bool),
    Status(StatusType),
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
            status: "Ready".to_string(),
            status_type: StatusType::Normal,
            input_tokens: 0,
            output_tokens: 0,
            last_input_tokens: 0,
            session_id: None,
            api_calls: 0,
            model: None,
            context_size: 0,
            tps: 0.0,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            selection_row: StatusBarRow::Runtime,
            selection_width: 0,
            context: StatusLineContext::default(),
            thinking: true,
        }
    }

    pub fn set_success(&mut self, status: &str) {
        self.status = status.to_string();
        self.status_type = StatusType::Success;
    }

    pub fn set_warning(&mut self, status: &str) {
        self.status = status.to_string();
        self.status_type = StatusType::Warning;
    }

    pub fn reset_runtime_state(&mut self) {
        log::debug!("[STATUS] reset_runtime_state()");
        self.status = "Ready".to_string();
        self.status_type = StatusType::Normal;
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.last_input_tokens = 0;
        self.api_calls = 0;
        self.tps = 0.0;
        self.clear_selection();
    }

    pub fn set_tokens(&mut self, input: u64, output: u64, last_input: u64) {
        self.input_tokens = input;
        self.output_tokens = output;
        self.last_input_tokens = last_input;
    }

    pub fn set_session_id(&mut self, id: &str) {
        self.session_id = Some(id.to_string());
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = Some(model.to_string());
    }

    pub fn set_context_size(&mut self, size: u64) {
        self.context_size = size;
    }

    pub fn set_api_calls(&mut self, count: u64) {
        self.api_calls = count;
    }

    pub fn set_tps(&mut self, tps: f64) {
        self.tps = tps;
    }

    pub fn set_thinking(&mut self, enabled: bool) {
        self.thinking = enabled;
    }

    pub fn set_current_dir(&mut self, dir: impl Into<String>) {
        let dir = dir.into();
        self.context.path_base = dir.clone();
        self.context.working_root = dir;
    }

    pub fn set_context_paths(
        &mut self,
        path_base: impl Into<String>,
        working_root: impl Into<String>,
    ) {
        self.context.path_base = path_base.into();
        self.context.working_root = working_root.into();
    }

    /// Set git checkout/worktree identity for the status context.
    pub fn set_git_context(&mut self, kind: WorktreeKind, branch: impl Into<String>) {
        let branch = branch.into();
        self.context.worktree_kind = kind;
        self.context.branch = if branch.trim().is_empty() {
            None
        } else {
            Some(branch)
        };
    }

    /// Set permission mode text for the status context.
    ///
    /// This is reserved for #42 PermissionEngine integration; until then the
    /// default status line shows AskMe.
    #[allow(dead_code)]
    pub fn set_permission_mode(&mut self, mode: impl Into<String>) {
        self.context.permission_mode = mode.into();
    }

    pub(crate) fn context_row_text(&self, width: usize) -> String {
        context_row_text(&self.context, width)
    }

    /// Render the status bar
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let base = Style::default().bg(theme::STATUS_BG);
        let runtime_area = Rect { height: 1, ..area };
        let runtime_line =
            if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
                self.runtime_row_spans_with_selection(start, end, base)
            } else {
                self.runtime_row_spans()
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
            let context_line = self.context_row_spans(area.width as usize, base);
            Paragraph::new(Line::from(context_line))
                .style(base)
                .render(context_area, buf);
        }
    }

    fn runtime_segments(&self) -> Vec<(String, RuntimeSegmentStyle)> {
        let mut segments = Vec::new();
        if let Some(ref model) = self.model {
            segments.push((format!(" {} ", model), RuntimeSegmentStyle::Model));
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        }

        segments.push((
            format!(" Think:{} │", if self.thinking { "ON" } else { "OFF" }),
            RuntimeSegmentStyle::Thinking(self.thinking),
        ));
        segments.push((
            format!(" {} ", self.status),
            RuntimeSegmentStyle::Status(self.status_type),
        ));
        segments.push((
            format!(
                " In: {} / Out: {} ",
                format_tokens(self.input_tokens),
                format_tokens(self.output_tokens)
            ),
            RuntimeSegmentStyle::Muted,
        ));
        if self.tps > 0.0 {
            segments.push((
                format!(" {:.0} t/s │", self.tps),
                RuntimeSegmentStyle::Border,
            ));
        }
        if self.context_size > 0 {
            let pct = self.context_pct();
            segments.push((
                format!("Ctx: {}% │", pct),
                RuntimeSegmentStyle::ContextPct(pct),
            ));
        } else {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Muted));
        }
        if let Some(ref id) = self.session_id {
            segments.push((
                format!(" Session: {} │ Calls: {} ", id, self.api_calls),
                RuntimeSegmentStyle::Muted,
            ));
        }
        segments
    }

    fn context_pct(&self) -> u64 {
        if self.last_input_tokens > 0 {
            self.last_input_tokens * 100 / self.context_size
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
            RuntimeSegmentStyle::Thinking(true) => Style::default().fg(theme::SUCCESS),
            RuntimeSegmentStyle::Thinking(false) => Style::default().fg(theme::TEXT_DIM),
            RuntimeSegmentStyle::Status(StatusType::Normal) => Style::default().fg(theme::TEXT),
            RuntimeSegmentStyle::Status(StatusType::Success) => Style::default().fg(theme::SUCCESS),
            RuntimeSegmentStyle::Status(StatusType::Warning) => Style::default().fg(theme::WARNING),
            RuntimeSegmentStyle::Muted => Style::default().fg(theme::TEXT_MUTED),
            RuntimeSegmentStyle::ContextPct(pct) if pct >= 80 => Style::default().fg(theme::ERROR),
            RuntimeSegmentStyle::ContextPct(pct) if pct >= 50 => {
                Style::default().fg(theme::WARNING)
            }
            RuntimeSegmentStyle::ContextPct(_) => Style::default().fg(theme::TEXT_MUTED),
        }
    }

    fn runtime_row_spans(&self) -> Vec<Span<'static>> {
        self.runtime_segments()
            .into_iter()
            .map(|(text, style)| Span::styled(text, self.runtime_segment_style(style)))
            .collect()
    }

    fn context_row_spans(&self, width: usize, base: Style) -> Vec<Span<'static>> {
        let text = self.context_row_text(width);
        if self.selection_row != StatusBarRow::Context {
            return vec![Span::styled(text, Style::default().fg(theme::TEXT_MUTED))];
        }
        self.spans_with_selection(text, base)
    }

    fn runtime_row_spans_with_selection(
        &self,
        _start: usize,
        _end: usize,
        base: Style,
    ) -> Vec<Span<'static>> {
        self.spans_with_selection(self.build_full_text(), base)
    }

    pub(crate) fn build_full_text(&self) -> String {
        self.runtime_segments()
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join("")
    }
}

#[cfg(test)]
#[path = "status_bar_tests.rs"]
mod tests;
