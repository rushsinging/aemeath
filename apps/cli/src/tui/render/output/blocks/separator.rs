use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
use std::rc::Rc;

pub fn render_separator(block_id: &str) -> RenderedBlock {
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(vec![RenderedLine::default()]),
    }
}
