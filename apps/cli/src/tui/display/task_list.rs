//! Task list and progress components
//!
//! Provides widgets for displaying task status and progress.

use crate::tui::display::theme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, StatefulWidget, Widget},
};

/// Task status
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    /// Get the display icon for this status
    pub fn icon(&self) -> &str {
        match self {
            TaskStatus::Pending => "○",
            TaskStatus::Running => "◉",
            TaskStatus::Completed => "✓",
            TaskStatus::Failed => "✗",
            TaskStatus::Cancelled => "⊘",
        }
    }

    /// Get the color for this status
    pub fn color(&self) -> Color {
        match self {
            TaskStatus::Pending => theme::TEXT_DIM,
            TaskStatus::Running => theme::TOOL_RUNNING,
            TaskStatus::Completed => theme::SUCCESS,
            TaskStatus::Failed => theme::ERROR,
            TaskStatus::Cancelled => theme::TEXT_MUTED,
        }
    }
}

/// A single task item
#[derive(Clone, Debug)]
pub struct TaskItem {
    /// Unique task ID
    pub id: String,
    /// Task subject/title
    pub subject: String,
    /// Current status
    pub status: TaskStatus,
    /// Optional description
    pub description: Option<String>,
    /// Progress percentage (0-100), if applicable
    pub progress: Option<u8>,
    /// Active form (shown when running)
    pub active_form: Option<String>,
}

impl TaskItem {
    pub fn new(id: &str, subject: &str) -> Self {
        Self {
            id: id.to_string(),
            subject: subject.to_string(),
            status: TaskStatus::Pending,
            description: None,
            progress: None,
            active_form: None,
        }
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    pub fn with_status(mut self, status: TaskStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_progress(mut self, progress: u8) -> Self {
        self.progress = Some(progress.min(100));
        self
    }

    pub fn with_active_form(mut self, form: &str) -> Self {
        self.active_form = Some(form.to_string());
        self
    }
}

/// A widget that displays a list of tasks
pub struct TaskList {
    /// List of tasks
    pub tasks: Vec<TaskItem>,
    /// Currently selected task index
    pub selected: Option<usize>,
    /// Show the task ID
    pub show_ids: bool,
    /// Compact mode (no descriptions)
    pub compact: bool,
}

impl TaskList {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            selected: None,
            show_ids: false,
            compact: false,
        }
    }

    pub fn with_tasks(mut self, tasks: Vec<TaskItem>) -> Self {
        self.tasks = tasks;
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = Some(index);
        self
    }

    /// Get the currently selected task
    pub fn get_selected_task(&self) -> Option<&TaskItem> {
        self.selected.and_then(|i| self.tasks.get(i))
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.tasks.is_empty() {
            self.selected = None;
            return;
        }
        match self.selected {
            Some(0) | None => {
                self.selected = Some(self.tasks.len() - 1);
            }
            Some(i) => {
                self.selected = Some(i - 1);
            }
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.tasks.is_empty() {
            self.selected = None;
            return;
        }
        match self.selected {
            Some(i) if i >= self.tasks.len() - 1 => {
                self.selected = Some(0);
            }
            Some(i) => {
                self.selected = Some(i + 1);
            }
            None => {
                self.selected = Some(0);
            }
        }
    }

    /// Render the task list
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                let mut spans = Vec::new();

                // Status icon
                spans.push(Span::styled(
                    format!("{} ", task.status.icon()),
                    Style::default().fg(task.status.color()),
                ));

                // Optional ID
                if self.show_ids {
                    spans.push(Span::styled(
                        format!("#{} ", task.id),
                        Style::default().fg(theme::TEXT_DIM),
                    ));
                }

                // Subject
                let subject_text = if task.status == TaskStatus::Running {
                    task.active_form.as_deref().unwrap_or(&task.subject)
                } else {
                    &task.subject
                };
                let subject_style = if self.selected == Some(i) {
                    Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT)
                };
                spans.push(Span::styled(subject_text, subject_style));

                // Progress bar
                if let Some(progress) = task.progress {
                    let bar_width = 10;
                    let filled = (progress as usize * bar_width) / 100;
                    let bar = format!(
                        " [{}{}]",
                        "█".repeat(filled),
                        "░".repeat(bar_width - filled)
                    );
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        bar,
                        Style::default().fg(theme::ACCENT),
                    ));
                    spans.push(Span::styled(
                        format!(" {}%", progress),
                        Style::default().fg(theme::TEXT_DIM),
                    ));
                }

                let line = Line::from(spans);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items);
        let mut state = ListState::default();
        state.select(self.selected);

        StatefulWidget::render(list, area, buf, &mut state);
    }
}

impl Default for TaskList {
    fn default() -> Self {
        Self::new()
    }
}

/// A progress bar widget
pub struct ProgressBar {
    /// Current progress (0-100)
    pub progress: u8,
    /// Bar width in characters
    pub width: u16,
    /// Show percentage label
    pub show_label: bool,
    /// Custom label
    pub label: Option<String>,
    /// Color for filled portion
    pub color: Color,
    /// Color for empty portion
    pub empty_color: Color,
}

impl ProgressBar {
    pub fn new(progress: u8) -> Self {
        Self {
            progress: progress.min(100),
            width: 20,
            show_label: true,
            label: None,
            color: theme::ACCENT,
            empty_color: theme::TEXT_DIM,
        }
    }

    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    pub fn label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Render the progress bar
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let filled = (self.progress as usize * self.width as usize) / 100;
        let empty = self.width as usize - filled;

        let mut spans = Vec::new();

        // Label
        if let Some(ref label) = self.label {
            spans.push(Span::styled(
                format!("{} ", label),
                Style::default().fg(theme::TEXT),
            ));
        }

        // Bar
        spans.push(Span::styled(
            "█".repeat(filled),
            Style::default().fg(self.color),
        ));
        spans.push(Span::styled(
            "░".repeat(empty),
            Style::default().fg(self.empty_color),
        ));

        // Percentage
        if self.show_label {
            spans.push(Span::styled(
                format!(" {}%", self.progress),
                Style::default().fg(theme::TEXT_DIM),
            ));
        }

        let line = Line::from(spans);
        let paragraph = ratatui::widgets::Paragraph::new(line);
        paragraph.render(area, buf);
    }
}

/// A compact status line showing multiple task statuses
pub struct TaskStatusLine {
    /// Tasks to display
    pub tasks: Vec<TaskItem>,
    /// Maximum number of tasks to show
    pub max_tasks: usize,
}

impl TaskStatusLine {
    pub fn new(tasks: Vec<TaskItem>) -> Self {
        Self {
            tasks,
            max_tasks: 5,
        }
    }

    /// Render the status line
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || self.tasks.is_empty() {
            return;
        }

        let mut spans = Vec::new();
        let running: Vec<_> = self
              .tasks
              .iter()
              .filter(|t| t.status == TaskStatus::Running)
              .collect();
        let completed = self.tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();
        let failed = self.tasks.iter().filter(|t| t.status == TaskStatus::Failed).count();

        // Running tasks
        if !running.is_empty() {
            spans.push(Span::styled(
                format!("{} running", running.len()),
                Style::default().fg(theme::TOOL_RUNNING),
            ));
        }

        // Completed
        if completed > 0 {
            if !spans.is_empty() {
                spans.push(Span::raw(" │ "));
            }
            spans.push(Span::styled(
                format!("✓ {} done", completed),
                Style::default().fg(theme::SUCCESS),
            ));
        }

        // Failed
        if failed > 0 {
            if !spans.is_empty() {
                spans.push(Span::raw(" │ "));
            }
            spans.push(Span::styled(
                format!("✗ {} failed", failed),
                Style::default().fg(theme::ERROR),
            ));
        }

        let line = Line::from(spans);
        let paragraph = ratatui::widgets::Paragraph::new(line);
        paragraph.render(area, buf);
    }
}