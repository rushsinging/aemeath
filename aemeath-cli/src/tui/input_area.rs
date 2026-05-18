use crate::tui::completion::Suggestion;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};
use tui_textarea::TextArea;

mod editing;
mod history;
mod render;
mod selection;
mod suggestions;

/// The input area with a multi-line text editor and autocomplete
pub struct InputArea {
    pub(super) textarea: TextArea<'static>,
    pub(super) focused: bool,
    pub(super) pending_images: usize,
    /// Active suggestions for autocomplete
    pub(super) suggestions: Vec<Suggestion>,
    /// Currently selected suggestion index (-1 means none)
    pub(super) selected_suggestion: i32,
    /// Whether suggestions are visible
    pub(super) show_suggestions: bool,
    /// Command history
    pub(super) history: Vec<String>,
    /// Current position in history (None means not browsing history)
    pub(super) history_index: Option<usize>,
    /// Saved input before browsing history (to restore when navigating back)
    pub(super) saved_input: String,
    /// 鼠标选中状态
    pub(super) is_selecting: bool,
    pub(super) selection_start: Option<(usize, usize)>, // (row, col) in textarea
    pub(super) selection_end: Option<(usize, usize)>,   // (row, col) in textarea
    /// textarea 渲染区域宽度（用于自动换行）
    pub(super) content_width: u16,
}

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}

impl InputArea {
    pub fn new() -> Self {
        Self {
            textarea: configured_textarea(),
            focused: true,
            pending_images: 0,
            suggestions: Vec::new(),
            selected_suggestion: -1,
            show_suggestions: false,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            content_width: 0,
        }
    }

    pub(super) fn hide_suggestions(&mut self) {
        log::debug!("hide_suggestions (was show={})", self.show_suggestions);
        self.show_suggestions = false;
    }

    /// Get the current input text
    pub fn get_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Clear the input
    pub fn clear(&mut self) {
        self.textarea = configured_textarea();
        self.clear_suggestions();
        self.reset_history_nav();
    }

    /// Set pending images count
    pub fn set_pending_images(&mut self, count: usize) {
        self.pending_images = count;
    }

    /// Check if input is empty
    pub fn is_empty(&self) -> bool {
        self.textarea.lines().iter().all(|line| line.is_empty())
    }

    /// Get cursor position (line, column)
    pub fn cursor_position(&self) -> (usize, usize) {
        self.textarea.cursor()
    }

    /// 获取 inner area（textarea 的实际渲染区域，去掉 border）
    pub fn get_inner_area(&self, area: &Rect) -> Rect {
        let block = Block::default().borders(Borders::ALL);
        block.inner(*area)
    }
}

fn configured_textarea() -> TextArea<'static> {
    let mut textarea = TextArea::default();
    textarea.set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
    textarea.set_cursor_line_style(Style::default().bg(Color::Reset));
    textarea.set_cursor_style(Style::default().bg(Color::Cyan));
    textarea
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    #[test]
    fn test_auto_wrap_current_line_handles_cjk_without_panic() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf);

        for ch in "你好世界你好世界".chars() {
            input.input(ch);
        }

        let text = input.get_text();
        assert!(text.contains('你'));
        assert!(text.contains('界'));
    }

    #[test]
    fn test_auto_wrap_current_line_handles_emoji_without_panic() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf);

        for ch in "a🚀b🚀c🚀d🚀e".chars() {
            input.input(ch);
        }

        let text = input.get_text();
        assert!(text.contains('🚀'));
        assert!(text.contains('e'));
    }
}
