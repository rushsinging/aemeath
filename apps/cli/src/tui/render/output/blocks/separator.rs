use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};

pub fn render_separator(block_id: &str) -> RenderedBlock {
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: vec![RenderedLine::default()],
    }
}
