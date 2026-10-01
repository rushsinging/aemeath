use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::input::model::InputModel;

use super::InputViewAssembler;

#[test]
fn test_input_assembler_reads_model_text_and_cursor() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertText("hello".to_string()));
    let vm = InputViewAssembler::assemble_from_model(&model, 0, true);
    assert_eq!(vm.text, "hello");
    assert_eq!(vm.cursor, 5);
}

#[test]
fn test_input_assembler_sets_placeholder_for_empty_input() {
    let model = InputModel::default();
    let vm = InputViewAssembler::assemble_from_model(&model, 0, true);
    assert!(vm.placeholder.is_some());
}

#[test]
fn test_input_assembler_shows_queued_hint() {
    let model = InputModel::default();
    let vm = InputViewAssembler::assemble_from_model(&model, 2, true);
    assert_eq!(vm.queued_hint.as_deref(), Some("已排队 2 条"));
}
