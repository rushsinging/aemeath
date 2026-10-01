use super::*;
use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
use ratatui::text::Span;
use std::rc::Rc;

#[test]
fn test_output_area_replace_document_replaces_content() {
    let mut area = OutputArea::new();
    let document = RenderedDocument::new(vec![RenderedBlock {
        block_id: "a".into(),
        lines: Rc::new(vec![RenderedLine::new(vec![Span::raw("x")])]),
    }]);
    area.replace_document(document);

    assert_eq!(area.document().total_lines(), 1);
}
