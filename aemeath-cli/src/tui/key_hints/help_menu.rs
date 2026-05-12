use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use std::collections::HashMap;

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
        sections.insert(
            "Navigation".to_string(),
            vec![
                ("PgUp/PgDn".to_string(), "Scroll up/down".to_string()),
                ("Home/End".to_string(), "Jump to start/end".to_string()),
                ("↑/↓".to_string(), "History navigation".to_string()),
            ],
        );
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

        let mut lines = vec![
            Line::styled(
                format!(" {} ", self.title),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                "─".repeat(area.width as usize),
                Style::default().fg(Color::DarkGray),
            ),
            Line::default(),
        ];

        for (section_name, items) in &self.sections {
            append_section(&mut lines, section_name, items);
        }
        lines.push(Line::styled(
            " Press Esc to close ",
            Style::default().fg(Color::DarkGray),
        ));

        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .render(area, buf);
    }
}

impl Default for HelpMenu {
    fn default() -> Self {
        Self::new()
    }
}

fn append_section<'a>(
    lines: &mut Vec<Line<'a>>,
    section_name: &str,
    items: &'a [(String, String)],
) {
    lines.push(Line::styled(
        format!(" {} ", section_name),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ));
    for (key, desc) in items {
        lines.push(Line::from(vec![
            Span::styled(format!("   {:15} ", key), Style::default().fg(Color::Cyan)),
            Span::styled(desc, Style::default().fg(Color::White)),
        ]));
    }
    lines.push(Line::default());
}
