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
pub struct InputArea;

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}

impl InputArea {
    pub fn new() -> Self {
        Self
    }

    #[cfg(test)]
    pub(super) fn hide_suggestions(&mut self) {}

    /// Get inner input render area, excluding border.
    pub fn get_inner_area(&self, area: &Rect) -> Rect {
        Self::inner_area(*area)
    }

    pub(crate) fn inner_area(area: Rect) -> Rect {
        let block = Block::default().borders(Borders::ALL);
        block.inner(area)
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
