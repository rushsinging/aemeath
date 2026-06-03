use crate::tui::render::theme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders},
};
use tui_textarea::TextArea;

mod editing;
mod history;
mod render;
mod resize;
mod selection;
pub mod suggestions;

/// The input area with a multi-line text editor and autocomplete
pub struct InputArea {
    pub(super) textarea: TextArea<'static>,
    pub(super) focused: bool,
    pub(super) pending_images: usize,
    /// Command history
    #[cfg(test)]
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
    pub(crate) content_width: u16,
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
            #[cfg(test)]
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            content_width: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn hide_suggestions(&mut self) {}

    /// Get the current input text.
    #[cfg(test)]
    pub fn get_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub(crate) fn text_snapshot(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Clear the input
    pub(crate) fn clear(&mut self) {
        self.textarea = configured_textarea();
        self.history_index = None;
        self.saved_input.clear();
    }

    /// Set pending images count
    pub(crate) fn set_pending_images(&mut self, count: usize) {
        self.pending_images = count;
    }

    /// Check if input is empty
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.textarea.lines().iter().all(|line| line.is_empty())
    }

    /// 光标是否在第一行（Up 翻历史的前置条件）
    pub(crate) fn cursor_is_at_first_line(&self) -> bool {
        let (row, _) = self.textarea.cursor();
        row == 0
    }

    /// 光标是否在最后一行（Down 翻历史的前置条件）
    pub(crate) fn cursor_is_at_last_line(&self) -> bool {
        let (row, _) = self.textarea.cursor();
        let total = self.textarea.lines().len();
        row + 1 >= total
    }

    /// 获取 inner area（textarea 的实际渲染区域，去掉 border）
    pub fn get_inner_area(&self, area: &Rect) -> Rect {
        let block = Block::default().borders(Borders::ALL);
        block.inner(*area)
    }

    /// 绘制输入区域 + 建议下拉（由外部决定areas布局）。
    pub fn draw(
        &mut self,
        area: Rect,
        suggestions_area: Rect,
        buf: &mut Buffer,
        pending_images: usize,
        suggestions: &suggestions::SuggestionViewState,
    ) {
        self.set_pending_images(pending_images);
        self.render(area, buf);
        if suggestions_area.height > 0 {
            self.render_suggestions_in_area(suggestions_area, buf, suggestions);
        }
    }
}

fn configured_textarea() -> TextArea<'static> {
    let mut textarea = TextArea::default();
    textarea.set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
    textarea.set_cursor_line_style(Style::default());
    textarea.set_cursor_style(Style::default().bg(theme::ACCENT).fg(theme::SURFACE));
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
