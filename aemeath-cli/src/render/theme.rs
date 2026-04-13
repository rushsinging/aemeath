use console::style;
use crossterm::style::Color;

/// Theme configuration for UI colors and styles
pub struct Theme;

impl Theme {
    pub const USER_PROMPT: Color = Color::Green;
    pub const TOOL_ERROR: Color = Color::Red;
    pub const INFO: Color = Color::DarkGrey;

    // Diff colors (Claude Code dark theme) — RGB tuples as single source of truth
    pub const DIFF_ADD_BG_RGB: (u8, u8, u8) = (0, 40, 10);
    pub const DIFF_ADD_FG_RGB: (u8, u8, u8) = (56, 166, 96);
    pub const DIFF_REMOVE_BG_RGB: (u8, u8, u8) = (60, 20, 30);
    pub const DIFF_REMOVE_FG_RGB: (u8, u8, u8) = (220, 100, 110);

    // crossterm Color accessors
    pub const DIFF_ADD_BG: Color = Color::Rgb { r: Self::DIFF_ADD_BG_RGB.0, g: Self::DIFF_ADD_BG_RGB.1, b: Self::DIFF_ADD_BG_RGB.2 };
    pub const DIFF_REMOVE_BG: Color = Color::Rgb { r: Self::DIFF_REMOVE_BG_RGB.0, g: Self::DIFF_REMOVE_BG_RGB.1, b: Self::DIFF_REMOVE_BG_RGB.2 };
    pub const DIFF_REMOVE_FG: Color = Color::Rgb { r: Self::DIFF_REMOVE_FG_RGB.0, g: Self::DIFF_REMOVE_FG_RGB.1, b: Self::DIFF_REMOVE_FG_RGB.2 };
}

/// Styled text helpers using console crate
pub struct StyledText;

impl StyledText {
    pub fn user_prompt(text: &str) -> String {
        format!("{}", style(text).green().bold())
    }

    pub fn tool_call(name: &str, summary: &str) -> String {
        format!("{} {}", style(format!("[{}]", name)).cyan().bold(), style(summary).white())
    }

    pub fn info(text: &str) -> String {
        format!("{}", style(text).dim())
    }

    pub fn warning(text: &str) -> String {
        format!("{}", style(text).yellow())
    }

    pub fn header(text: &str) -> String {
        format!("{}", style(text).magenta().bold())
    }

    pub fn success(text: &str) -> String {
        format!("{} {}", style("✓").green().bold(), style(text).green())
    }

    pub fn error(text: &str) -> String {
        format!("{} {}", style("✗").red().bold(), style(text).red())
    }

    pub fn separator() -> String {
        format!("{}", style("─".repeat(60)).dim())
    }

    pub fn highlight(text: &str) -> String {
        format!("{}", style(text).cyan().bold())
    }
}
