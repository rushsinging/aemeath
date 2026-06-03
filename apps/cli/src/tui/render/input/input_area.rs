use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Borders},
};

mod editing;
mod history;
mod render;
mod resize;
mod selection;
pub mod suggestions;

/// The input area with a multi-line text editor and autocomplete
pub struct InputArea {
    pub(super) focused: bool,
    pub(super) pending_images: usize,
    /// Command history mirror is kept for test-only widget history helpers; production history
    /// truth lives in `InputModel::history`.
    #[cfg(test)]
    pub(super) history: Vec<String>,
    /// Current position in history (None means not browsing history)
    pub(super) history_index: Option<usize>,
    /// Saved input before browsing history (to restore when navigating back)
    pub(super) saved_input: String,
    /// Input render content width cache; not a text/cursor truth.
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
            focused: true,
            pending_images: 0,
            #[cfg(test)]
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            content_width: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn hide_suggestions(&mut self) {}

    /// Clear non-text input widget mirrors.
    pub(crate) fn clear(&mut self) {
        self.history_index = None;
        self.saved_input.clear();
    }

    /// Set pending images count
    pub(crate) fn set_pending_images(&mut self, count: usize) {
        self.pending_images = count;
    }

    /// Get inner input render area, excluding border.
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
        render_model: &crate::tui::render::input::input_render_model::InputRenderModel,
        selection: &crate::tui::view_state::InputSelectionViewState,
        suggestions: &suggestions::SuggestionViewState,
    ) {
        self.render(area, buf, render_model, selection);
        if suggestions_area.height > 0 {
            self.render_suggestions_in_area(suggestions_area, buf, suggestions);
        }
    }
}
