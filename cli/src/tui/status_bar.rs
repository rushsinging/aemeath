#[path = "status_bar_format.rs"]
mod status_bar_format;

use crate::tui::safe_text::{col_to_char_idx, safe_char_slice};
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
            Paragraph::new(self.context_row_text(area.width as usize))
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

    fn runtime_row_spans_with_selection(
        &self,
        start: usize,
        end: usize,
        base: Style,
    ) -> Vec<Span<'static>> {
        let (start, end) = if start < end {
            (start, end)
        } else {
            (end, start)
        };
        if start == end {
            return self.runtime_row_spans();
        }

        let full_text = self.build_full_text();
        let chars: Vec<char> = full_text.chars().collect();
        let len = chars.len();
        let before: String = safe_char_slice(&chars, 0, start.min(len)).iter().collect();
        let selected: String = safe_char_slice(&chars, start.min(len), end.min(len))
            .iter()
            .collect();
        let after: String = safe_char_slice(&chars, end.min(len), len).iter().collect();
        let selection_style = Style::default()
            .bg(theme::SELECTION_BG)
            .fg(theme::SELECTION_FG);
        let mut highlighted = Vec::new();
        if !before.is_empty() {
            highlighted.push(Span::styled(before, base));
        }
        if !selected.is_empty() {
            highlighted.push(Span::styled(selected, selection_style));
        }
        if !after.is_empty() {
            highlighted.push(Span::styled(after, base));
        }
        highlighted
    }

    fn build_full_text(&self) -> String {
        self.runtime_segments()
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn start_selection(&mut self, col: u16) {
        self.selection_start = Some(self.screen_col_to_char_idx(col));
        self.selection_end = Some(self.screen_col_to_char_idx(col));
        self.is_selecting = true;
    }

    pub fn update_selection(&mut self, col: u16) {
        if self.is_selecting {
            self.selection_end = Some(self.screen_col_to_char_idx(col));
        }
    }

    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        self.selection_start = None;
        self.selection_end = None;
        text
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        let (start, end) = if start < end {
            (start, end)
        } else {
            (end, start)
        };
        if start == end {
            return None;
        }
        let full = self.build_full_text();
        let chars: Vec<char> = full.chars().collect();
        let selected: String = chars[start.min(chars.len())..end.min(chars.len())]
            .iter()
            .collect();
        if selected.is_empty() {
            None
        } else {
            Some(selected)
        }
    }

    fn screen_col_to_char_idx(&self, col: u16) -> usize {
        col_to_char_idx(&self.build_full_text(), col as usize)
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }
}

#[cfg(test)]
#[path = "status_bar_tests.rs"]
mod tests;
