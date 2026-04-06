//! Dialog components for user interactions
//!
//! Provides modal dialogs for confirmations, selections, and user input.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

/// Dialog type
#[derive(Clone, Debug)]
pub enum DialogType {
    /// Confirmation dialog with Yes/No options
    Confirm { title: String, message: String },
    /// Information dialog with OK button
    Info { title: String, message: String },
    /// Error dialog
    Error { title: String, message: String },
    /// Selection dialog with multiple options
    Select { title: String, options: Vec<String>, selected: usize },
}

/// A modal dialog widget
pub struct Dialog {
    /// Dialog type and content
    pub dialog_type: DialogType,
    /// Whether the dialog is visible
    pub visible: bool,
    /// Width of the dialog (0 = auto)
    pub width: u16,
}

impl Dialog {
    /// Create a new confirmation dialog
    pub fn confirm(title: &str, message: &str) -> Self {
        Self {
            dialog_type: DialogType::Confirm {
                title: title.to_string(),
                message: message.to_string(),
            },
            visible: true,
            width: 0,
        }
    }

    /// Create a new info dialog
    pub fn info(title: &str, message: &str) -> Self {
        Self {
            dialog_type: DialogType::Info {
                title: title.to_string(),
                message: message.to_string(),
            },
            visible: true,
            width: 0,
        }
    }

    /// Create a new error dialog
    pub fn error(title: &str, message: &str) -> Self {
        Self {
            dialog_type: DialogType::Error {
                title: title.to_string(),
                message: message.to_string(),
            },
            visible: true,
            width: 0,
        }
    }

    /// Create a new selection dialog
    pub fn select(title: &str, options: Vec<String>) -> Self {
        Self {
            dialog_type: DialogType::Select {
                title: title.to_string(),
                options,
                selected: 0,
            },
            visible: true,
            width: 0,
        }
    }

    /// Set the selected option (for Select dialogs)
    pub fn set_selected(&mut self, index: usize) {
        if let DialogType::Select { selected, .. } = &mut self.dialog_type {
            *selected = index;
        }
    }

    /// Get the selected option index
    pub fn get_selected(&self) -> Option<usize> {
        if let DialogType::Select { selected, .. } = &self.dialog_type {
            Some(*selected)
        } else {
            None
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if let DialogType::Select { options, selected, .. } = &mut self.dialog_type {
            if *selected > 0 {
                *selected -= 1;
            } else {
                *selected = options.len().saturating_sub(1);
            }
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if let DialogType::Select { options, selected, .. } = &mut self.dialog_type {
            *selected = (*selected + 1) % options.len().max(1);
        }
    }

    /// Calculate dialog dimensions
    fn calculate_dimensions(&self, area: Rect) -> Rect {
        let (title_str, message_str, options_count): (&str, &str, usize) = match &self.dialog_type {
            DialogType::Confirm { title, message } => (title.as_str(), message.as_str(), 2),
            DialogType::Info { title, message } => (title.as_str(), message.as_str(), 1),
            DialogType::Error { title, message } => (title.as_str(), message.as_str(), 1),
            DialogType::Select { title, options, .. } => {
                (title.as_str(), "", options.len())
            }
        };

        // Calculate width
        let content_width = if self.width > 0 {
            self.width
        } else {
            let msg_len = message_str.len().max(title_str.len()) as u16;
            let options_len = options_count.max(1) as u16 * 20;
            (msg_len.max(options_len) + 4).min(area.width.saturating_sub(4))
        };

        // Calculate height
        let message_lines = if message_str.is_empty() { 0 } else { 1 };
        let height = (3 + message_lines + options_count as u16).min(area.height.saturating_sub(2));

        // Center the dialog
        let x = (area.width.saturating_sub(content_width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;

        Rect::new(x, y, content_width, height)
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible {
            return;
        }

        let dialog_area = self.calculate_dimensions(area);

        // Clear the area under the dialog
        Clear.render(dialog_area, buf);

        // Determine colors based on dialog type
        let (border_color, title_style) = match &self.dialog_type {
            DialogType::Confirm { .. } => (Color::Yellow, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            DialogType::Info { .. } => (Color::Blue, Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            DialogType::Error { .. } => (Color::Red, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            DialogType::Select { .. } => (Color::Cyan, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        };

        // Render dialog border
        let (title, message) = match &self.dialog_type {
            DialogType::Confirm { title, message } => (title.as_str(), message.as_str()),
            DialogType::Info { title, message } => (title.as_str(), message.as_str()),
            DialogType::Error { title, message } => (title.as_str(), message.as_str()),
            DialogType::Select { title, .. } => (title.as_str(), ""),
        };

        let block = Block::default()
            .title(Span::styled(format!(" {} ", title), title_style))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Render content
        let mut lines = Vec::new();

        // Message
        if !message.is_empty() {
            lines.push(Line::styled(message, Style::default().fg(Color::White)));
            lines.push(Line::default()); // Empty line
        }

        // Options/buttons
        match &self.dialog_type {
            DialogType::Confirm { .. } => {
                lines.push(Line::from(vec![
                    Span::styled(" [Y]es ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::raw("   "),
                    Span::styled(" [N]o ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                ]));
            }
            DialogType::Info { .. } | DialogType::Error { .. } => {
                lines.push(Line::styled(
                    " [Enter] OK ",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
            }
            DialogType::Select { options, selected, .. } => {
                for (i, option) in options.iter().enumerate() {
                    if i == *selected {
                        lines.push(Line::styled(
                            format!(" ❯ {}", option),
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ));
                    } else {
                        lines.push(Line::styled(
                            format!("   {}", option),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
            }
        }

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Center);
        paragraph.render(inner, buf);
    }
}

/// A simple notification/toast widget
pub struct Notification {
    pub message: String,
    pub notification_type: NotificationType,
    pub visible: bool,
    /// Auto-hide timeout in milliseconds (0 = no auto-hide)
    pub timeout_ms: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum NotificationType {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl Notification {
    pub fn info(message: &str) -> Self {
        Self {
            message: message.to_string(),
            notification_type: NotificationType::Info,
            visible: true,
            timeout_ms: 3000,
        }
    }

    pub fn success(message: &str) -> Self {
        Self {
            message: message.to_string(),
            notification_type: NotificationType::Success,
            visible: true,
            timeout_ms: 3000,
        }
    }

    pub fn warning(message: &str) -> Self {
        Self {
            message: message.to_string(),
            notification_type: NotificationType::Warning,
            visible: true,
            timeout_ms: 5000,
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            message: message.to_string(),
            notification_type: NotificationType::Error,
            visible: true,
            timeout_ms: 0, // Errors don't auto-hide
        }
    }

    /// Render the notification at the top of the screen
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible {
            return;
        }

        let (icon, color) = match self.notification_type {
            NotificationType::Info => ("ℹ", Color::Blue),
            NotificationType::Success => ("✓", Color::Green),
            NotificationType::Warning => ("⚠", Color::Yellow),
            NotificationType::Error => ("✗", Color::Red),
        };

        let text = format!(" {} {}", icon, self.message);
        let width = (text.len() as u16 + 2).min(area.width);

        let notif_area = Rect::new(
            (area.width.saturating_sub(width)) / 2,
            1,
            width,
            1,
        );

        let line = Line::styled(
            format!(" {} ", text),
            Style::default().fg(Color::White).bg(color),
        );

        let paragraph = Paragraph::new(line);
        paragraph.render(notif_area, buf);
    }
}