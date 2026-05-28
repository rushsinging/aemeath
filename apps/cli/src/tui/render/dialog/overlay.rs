//! Dialog components for user interactions
//!
//! Provides a modal selection dialog for the TUI.

use crate::tui::render::theme;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

/// A modal selection dialog widget
pub struct Dialog {
    title: String,
    options: Vec<String>,
    selected: usize,
    /// Whether the dialog is visible
    pub visible: bool,
}

impl Dialog {
    /// Create a new selection dialog
    pub fn select(title: &str, options: Vec<String>) -> Self {
        Self {
            title: title.to_string(),
            options,
            selected: 0,
            visible: true,
        }
    }

    /// Get the selected option index
    pub fn get_selected(&self) -> Option<usize> {
        if self.options.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    /// Move selection up (wraps around)
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.options.len().saturating_sub(1);
        }
    }

    /// Move selection down (wraps around)
    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.options.len().max(1);
    }

    /// Render the dialog centered on the screen
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || self.options.is_empty() {
            return;
        }

        // Calculate dimensions
        let max_option_len = self.options.iter().map(|o| o.len()).max().unwrap_or(20);
        let content_width = (max_option_len as u16 + 8)
            .max(self.title.len() as u16 + 6)
            .min(area.width.saturating_sub(4));
        let height = (self.options.len() as u16 + 3) // +3 for border + hint line
            .min(area.height.saturating_sub(2));

        // Center
        let x = (area.width.saturating_sub(content_width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect::new(x, y, content_width, height);

        // Clear background
        Clear.render(dialog_area, buf);

        // Border
        let block = Block::default()
            .title(Span::styled(
                format!(" {} ", self.title),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Options
        let mut lines: Vec<Line> = Vec::new();
        for (i, option) in self.options.iter().enumerate() {
            if i == self.selected {
                lines.push(Line::styled(
                    format!(" > {}", option),
                    Style::default()
                        .fg(theme::WARNING)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                lines.push(Line::styled(
                    format!("   {}", option),
                    Style::default().fg(theme::TEXT_MUTED),
                ));
            }
        }

        // Hint line
        lines.push(Line::styled(
            " Enter=select  Esc=cancel",
            Style::default().fg(theme::TEXT_DIM),
        ));

        let paragraph = Paragraph::new(lines).alignment(Alignment::Left);
        paragraph.render(inner, buf);
    }
}
