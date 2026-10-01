use super::*;
use crate::tui::model::input::document::InputDocument;
use crate::tui::view_assembler::input::InputViewAssembler;

fn vm(text: &str) -> InputAreaViewModel {
    let mut document = InputDocument::default();
    document.insert_text(text);
    InputViewAssembler::from_document(&document, None, true)
}

#[test]
fn input_content_width_accounts_for_border() {
    assert_eq!(InputArea::input_content_width(80), 78);
}

#[test]
fn input_content_width_saturates_small_width() {
    assert_eq!(InputArea::input_content_width(1), 0);
}

#[test]
fn desired_height_grows_with_wrapped_input_lines() {
    assert_eq!(InputArea::desired_height(6, &vm("abcdef")), 4);
}

#[test]
fn desired_height_keeps_minimum_for_empty_input() {
    assert_eq!(InputArea::desired_height(80, &vm("")), 3);
}
