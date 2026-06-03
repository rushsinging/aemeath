#[path = "../display/status_bar_format.rs"]
mod status_bar_format;
#[path = "../display/status_bar_selection.rs"]
mod status_bar_selection;

use crate::tui::render::theme;
use crate::tui::view_model::StatusRuntimeViewModel;
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
/// 运行态镜像（token/tps/model/session/context）统一收敛到 `vm`，唯一写入路径为
/// `apply_runtime_view`（由 `adapter/status_widget.rs` 调用，源自 `StatusViewAssembler`）。
/// 渲染只读 `vm`，update 业务路径禁止直接调用 `set_*`。
pub struct StatusBar {
    status: String,
    status_type: StatusType,
    vm: StatusRuntimeViewModel,
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
            vm: StatusRuntimeViewModel::default(),
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
        self.clear_selection();
    }

    /// 由 adapter 依据 `StatusViewAssembler` 派生结果单向写回 widget 镜像
    /// （model/session/tps/token/api/context_size/工作目录上下文）。
    ///
    /// 这是上述镜像的**唯一**生产写入路径；update 业务路径禁止直接调用对应 `set_*`
    /// （由结构性 status single-source guard 焊死）。permission_mode 为启动期配置，
    /// 在此保留不被覆盖。
    pub(crate) fn apply_runtime_view(&mut self, view: StatusRuntimeViewModel) {
        self.vm = view;
        self.context.path_base = self.vm.context.path_base.clone();
        self.context.working_root = self.vm.context.working_root.clone();
        self.context.worktree_kind = match self.vm.context.kind {
            crate::tui::view_model::StatusWorktreeKind::Worktree => WorktreeKind::Worktree,
            crate::tui::view_model::StatusWorktreeKind::Main => WorktreeKind::Main,
        };
        self.context.branch = self.vm.context.branch.clone();
        self.context.session_id = self.vm.session_id.clone();
    }

    pub fn set_thinking(&mut self, enabled: bool) {
        self.thinking = enabled;
    }

    #[cfg(test)]
    pub fn set_current_dir(&mut self, dir: impl Into<String>) {
        let dir = dir.into();
        self.context.path_base = dir.clone();
        self.context.working_root = dir;
    }

    /// 一次性初始化工作目录上下文。
    pub fn init(&mut self, _session_id: &str, _model: &str, cwd: &std::path::Path) {
        let cwd_display = crate::tui::app::display_status_path(cwd);
        self.context.path_base = cwd_display.clone();
        self.context.working_root = cwd_display;
        if let Some(branch) = crate::tui::app::git_branch_for(cwd) {
            let branch = branch.trim().to_string();
            self.context.worktree_kind = crate::tui::app::worktree_kind_for(cwd);
            self.context.branch = if branch.is_empty() {
                None
            } else {
                Some(branch)
            };
        }
    }

    /// 绘制状态栏。
    pub fn draw(&self, area: Rect, buf: &mut Buffer) {
        self.render(area, buf);
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
        segments.push((
            format!(" {} ", self.status),
            RuntimeSegmentStyle::Status(self.status_type),
        ));
        if let Some(ref model) = self.vm.model {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" {} ", model), RuntimeSegmentStyle::Model));
        }
        segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        segments.push((
            format!(" in {} ", sdk::format_tokens(self.vm.input_tokens)),
            RuntimeSegmentStyle::Muted,
        ));
        segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        segments.push((
            format!(" out {} ", sdk::format_tokens(self.vm.output_tokens)),
            RuntimeSegmentStyle::Muted,
        ));
        if self.vm.tps > 0.0 {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((
                format!(" {:.0} t/s ", self.vm.tps),
                RuntimeSegmentStyle::Muted,
            ));
        }
        if self.vm.context_size > 0 {
            let pct = self.context_pct();
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((
                format!(" ctx {}% ", pct),
                RuntimeSegmentStyle::ContextPct(pct),
            ));
        }
        if self.vm.api_calls > 0 {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((
                format!(" api {} ", self.vm.api_calls),
                RuntimeSegmentStyle::Muted,
            ));
        }
        segments
    }

    fn context_pct(&self) -> u64 {
        if self.vm.last_input_tokens > 0 {
            self.vm.last_input_tokens * 100 / self.vm.context_size
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
        if self.selection_row == StatusBarRow::Context {
            return self.spans_with_selection(text, base);
        }
        let parts: Vec<&str> = text.split(" │ ").collect();
        let has_root = parts.get(1).is_some_and(|part| part.starts_with("root "));
        let git_index = 1 + usize::from(has_root);
        let permission_index = git_index + 1;
        let mut spans = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER)));
            }
            let style = if index == 0 {
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if has_root && index == 1 {
                Style::default().fg(theme::TEXT_MUTED)
            } else if index == git_index {
                Style::default().fg(theme::SUCCESS)
            } else if index == permission_index {
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
#[path = "../display/status_bar_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "../display/status_bar_v2_tests.rs"]
mod v2_tests;
