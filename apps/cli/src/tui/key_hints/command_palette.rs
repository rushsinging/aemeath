use crate::tui::theme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

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
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.commands.get(i))
    }

    /// Render the command palette
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible || area.height == 0 {
            return;
        }

        let mut lines = vec![search_prompt(&self.query), separator(area.width)];
        for (display_idx, &cmd_idx) in self.filtered.iter().enumerate() {
            if display_idx >= (area.height as usize).saturating_sub(4) {
                break;
            }
            lines.push(command_line(
                &self.commands[cmd_idx],
                display_idx == self.selected,
            ));
        }
        if !self.filtered.is_empty() {
            lines.push(Line::default());
            lines.push(Line::styled(
                " Enter to execute, Esc to close ",
                Style::default().fg(theme::TEXT_DIM),
            ));
        }

        Paragraph::new(lines).render(area, buf);
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

fn search_prompt(query: &str) -> Line<'_> {
    Line::from(vec![
        Span::styled(" ❯ ", Style::default().fg(theme::ACCENT)),
        Span::styled(query, Style::default().fg(theme::TEXT)),
        Span::raw("_"),
    ])
}

fn separator(width: u16) -> Line<'static> {
    Line::styled("─".repeat(width as usize), Style::default().fg(theme::TEXT_DIM))
}

fn command_line(cmd: &Command, is_selected: bool) -> Line<'_> {
    let name_style = if is_selected {
        Style::default().fg(theme::WARNING).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT)
    };
    Line::from(vec![
        Span::styled(
            if is_selected { " ❯ " } else { "   " },
            Style::default().fg(theme::WARNING),
        ),
        Span::styled(&cmd.name, name_style),
        Span::raw(" - "),
        Span::styled(&cmd.description, Style::default().fg(theme::TEXT_DIM)),
    ])
}
