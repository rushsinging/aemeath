//! Keyboard shortcut hint components
//!
//! Provides widgets for displaying keyboard shortcuts and help information.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use std::collections::HashMap;

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
            // Group separator
            if self.show_groups {
                if hint.group.as_deref() != current_group {
                    if let Some(group) = &hint.group {
                        if !spans.is_empty() {
                            spans.push(Span::raw("  "));
                        }
                        spans.push(Span::styled(
                            format!("[{}] ", group),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    current_group = hint.group.as_deref();
                }
            }

            if !spans.is_empty() {
                spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
            }

            // Key
            spans.push(Span::styled(
                format!(" {} ", hint.key),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

            // Description
            spans.push(Span::styled(
                hint.description.as_str(),
                Style::default().fg(Color::DarkGray),
            ));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line);
        paragraph.render(area, buf);
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
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(hint.description.as_str(), Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
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

/// A help menu widget
pub struct HelpMenu {
    /// Title of the help menu
    pub title: String,
    /// Content sections
    pub sections: HashMap<String, Vec<(String, String)>>,
    /// Whether the menu is visible
    pub visible: bool,
}

impl HelpMenu {
    pub fn new() -> Self {
        let mut sections = HashMap::new();

        // Navigation
        sections.insert(
            "Navigation".to_string(),
            vec![
                ("PgUp/PgDn".to_string(), "Scroll up/down".to_string()),
                ("Home/End".to_string(), "Jump to start/end".to_string()),
                ("↑/↓".to_string(), "History navigation".to_string()),
            ],
        );

        // Input
        sections.insert(
            "Input".to_string(),
            vec![
                ("Enter".to_string(), "Send message".to_string()),
                ("Alt+Enter".to_string(), "Insert newline".to_string()),
                ("Ctrl+W".to_string(), "Delete word".to_string()),
                ("Ctrl+V".to_string(), "Paste image from clipboard".to_string()),
                ("Tab".to_string(), "Accept suggestion".to_string()),
            ],
        );

        // Commands
        sections.insert(
            "Commands".to_string(),
            vec![
                ("/help".to_string(), "Show this help".to_string()),
                ("/exit".to_string(), "Exit application".to_string()),
                ("/clear".to_string(), "Clear conversation".to_string()),
                ("/usage".to_string(), "Show token usage".to_string()),
                ("/paste".to_string(), "Paste clipboard image".to_string()),
                ("/images".to_string(), "List pending images".to_string()),
            ],
        );

        // Actions
        sections.insert(
            "Actions".to_string(),
            vec![
                ("Ctrl+C".to_string(), "Interrupt or exit".to_string()),
                ("Esc".to_string(), "Dismiss dialogs/suggestions".to_string()),
            ],
        );

        Self {
            title: "Help".to_string(),
            sections,
            visible: true,
        }
    }

    /// Render the help menu
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible {
            return;
        }

        let mut lines = Vec::new();

        // Title
        lines.push(Line::styled(
            format!(" {} ", self.title),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::default());

        // Sections
        for (section_name, items) in &self.sections {
            // Section header
            lines.push(Line::styled(
                format!(" {} ", section_name),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));

            // Items
            for (key, desc) in items {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("   {:15} ", key),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(desc, Style::default().fg(Color::White)),
                ]));
            }

            lines.push(Line::default());
        }

        // Footer
        lines.push(Line::styled(
            " Press Esc to close ",
            Style::default().fg(Color::DarkGray),
        ));

        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left);
        paragraph.render(area, buf);
    }
}

impl Default for HelpMenu {
    fn default() -> Self {
        Self::new()
    }
}

/// A simple command palette widget
pub struct CommandPalette {
    /// Available commands
    pub commands: Vec<Command>,
    /// Filtered commands based on search
    pub filtered: Vec<usize>,
    /// Current search query
    pub query: String,
    /// Currently selected command
    pub selected: usize,
    /// Whether the palette is visible
    pub visible: bool,
}

/// A command that can be executed
#[derive(Clone, Debug)]
pub struct Command {
    /// Command name (what to type)
    pub name: String,
    /// Display description
    pub description: String,
    /// Keyboard shortcut (if any)
    pub shortcut: Option<String>,
    /// Category for grouping
    pub category: String,
}

impl Command {
    pub fn new(name: &str, description: &str, category: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            shortcut: None,
            category: category.to_string(),
        }
    }

    pub fn shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.to_string());
        self
    }
}

impl CommandPalette {
    pub fn new() -> Self {
        let commands = vec![
            Command::new("help", "Show help menu", "General"),
            Command::new("exit", "Exit the application", "General"),
            Command::new("clear", "Clear the conversation", "General"),
            Command::new("usage", "Show token usage statistics", "General"),
            Command::new("paste", "Paste image from clipboard", "Input"),
            Command::new("images", "List pending images", "Input"),
            Command::new("clear-images", "Clear pending images", "Input"),
        ];

        Self {
            filtered: (0..commands.len()).collect(),
            commands,
            query: String::new(),
            selected: 0,
            visible: false,
        }
    }

    /// Update the search query and filter commands
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_lowercase();
        self.filtered = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| {
                self.query.is_empty()
                    || cmd.name.to_lowercase().contains(&self.query)
                    || cmd.description.to_lowercase().contains(&self.query)
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.filtered.len() - 1
        } else {
            self.selected - 1
        };
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
    }

    /// Get the currently selected command
    pub fn get_selected(&self) -> Option<&Command> {
        self.filtered.get(self.selected).and_then(|&i| self.commands.get(i))
    }

    /// Render the command palette
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || area.height == 0 {
            return;
        }

        let mut lines = Vec::new();

        // Search prompt
        lines.push(Line::from(vec![
            Span::styled(" ❯ ", Style::default().fg(Color::Cyan)),
            Span::styled(&self.query, Style::default().fg(Color::White)),
            Span::raw("_"), // Cursor
        ]));

        lines.push(Line::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        ));

        // Commands
        for (display_idx, &cmd_idx) in self.filtered.iter().enumerate() {
            if display_idx >= (area.height as usize).saturating_sub(4) {
                break;
            }

            let cmd = &self.commands[cmd_idx];
            let is_selected = display_idx == self.selected;

            let mut spans = Vec::new();

            // Selection indicator
            spans.push(Span::styled(
                if is_selected { " ❯ " } else { "   " },
                Style::default().fg(Color::Yellow),
            ));

            // Command name
            spans.push(Span::styled(
                &cmd.name,
                if is_selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ));

            // Description
            spans.push(Span::raw(" - "));
            spans.push(Span::styled(
                &cmd.description,
                Style::default().fg(Color::DarkGray),
            ));

            lines.push(Line::from(spans));
        }

        // Footer
        if !self.filtered.is_empty() {
            lines.push(Line::default());
            lines.push(Line::styled(
                " Enter to execute, Esc to close ",
                Style::default().fg(Color::DarkGray),
            ));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}