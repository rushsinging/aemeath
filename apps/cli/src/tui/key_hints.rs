//! Keyboard shortcut hint components
//!
//! Provides widgets for displaying keyboard shortcuts and help information.

use crate::tui::theme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

mod command_palette;
mod help_menu;

pub use command_palette::{Command, CommandPalette};
pub use help_menu::HelpMenu;

/// A keyboard shortcut hint
#[derive(Clone, Debug)]
pub struct KeyHint {
    /// The key combination (e.g., "Ctrl+C", "Enter")
    pub key: String,
    /// Description of what the key does
    pub description: String,
    /// Group name for organization
    pub group: Option<String>,
}

impl KeyHint {
    pub fn new(key: &str, description: &str) -> Self {
        Self {
            key: key.to_string(),
            description: description.to_string(),
            group: None,
        }
    }

    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }
}

/// A widget that displays keyboard shortcuts
pub struct KeyHints {
    /// List of key hints to display
    hints: Vec<KeyHint>,
    /// Whether to show group names
    show_groups: bool,
    /// Maximum width per hint
    max_hint_width: usize,
}

impl Default for KeyHints {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyHints {
    pub fn new() -> Self {
        Self {
            hints: Vec::new(),
            show_groups: false,
            max_hint_width: 20,
        }
    }

    pub fn hint(mut self, key: &str, description: &str) -> Self {
        self.hints.push(KeyHint::new(key, description));
        self
    }

    pub fn with_hints(mut self, hints: Vec<KeyHint>) -> Self {
        self.hints = hints;
        self
    }

    pub fn show_groups(mut self, show: bool) -> Self {
        self.show_groups = show;
        self
    }

    /// Render as a horizontal bar (compact)
    pub fn render_horizontal(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let mut spans = Vec::new();
        let mut current_group: Option<&str> = None;
        for hint in &self.hints {
            append_group_separator(&mut spans, hint, self.show_groups, &mut current_group);
            if !spans.is_empty() {
                spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER)));
            }
            spans.extend(key_description_spans(hint));
        }

        Paragraph::new(Line::from(spans)).render(area, buf);
    }

    /// Render as a vertical list (for help menu)
    pub fn render_vertical(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        let lines: Vec<Line> = self
            .hints
            .iter()
            .map(|hint| {
                Line::from(vec![
                    Span::styled(
                        format!(" {:12} ", hint.key),
                        Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(hint.description.as_str(), Style::default().fg(theme::TEXT)),
                ])
            })
            .collect();
        Paragraph::new(lines).render(area, buf);
    }
}

fn append_group_separator<'a>(
    spans: &mut Vec<Span<'a>>,
    hint: &'a KeyHint,
    show_groups: bool,
    current_group: &mut Option<&'a str>,
) {
    if !show_groups || hint.group.as_deref() == *current_group {
        return;
    }
    if let Some(group) = &hint.group {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!("[{}] ", group),
            Style::default().fg(theme::BORDER),
        ));
    }
    *current_group = hint.group.as_deref();
}

fn key_description_spans(hint: &KeyHint) -> Vec<Span<'_>> {
    vec![
        Span::styled(
            format!(" {} ", hint.key),
            Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(hint.description.as_str(), Style::default().fg(theme::BORDER)),
    ]
}

/// Default key bindings for the TUI
pub fn default_key_hints() -> Vec<KeyHint> {
    vec![
        KeyHint::new("Enter", "Send message"),
        KeyHint::new("Alt+Enter", "New line"),
        KeyHint::new("Ctrl+C", "Interrupt/Exit"),
        KeyHint::new("Ctrl+V", "Paste image"),
        KeyHint::new("Tab", "Accept suggestion"),
        KeyHint::new("Esc", "Dismiss"),
        KeyHint::new("PgUp/Dn", "Scroll"),
        KeyHint::new("/help", "Show commands"),
    ]
}
