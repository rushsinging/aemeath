use crate::tui::model::input::model::InputModel;
use crate::tui::view_model::InputAreaViewModel;

pub struct InputViewAssembler;

impl InputViewAssembler {
    pub fn assemble_from_model(
        model: &InputModel,
        queued_count: usize,
        pending_images: usize,
        focused: bool,
    ) -> InputAreaViewModel {
        let mut vm = InputAreaViewModel::from_document(
            &model.document,
            model
                .document
                .buffer
                .is_empty()
                .then(|| "输入消息...".to_string()),
            pending_images,
            focused,
        );
        vm.queued_hint = (queued_count > 0).then(|| format!("已排队 {queued_count} 条"));
        vm
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::model::input::intent::InputIntent;
    use crate::tui::model::input::model::InputModel;

    use super::InputViewAssembler;

    #[test]
    fn test_input_assembler_reads_model_text_and_cursor() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("hello".to_string()));
        let vm = InputViewAssembler::assemble_from_model(&model, 0, 0, true);
        assert_eq!(vm.text, "hello");
        assert_eq!(vm.cursor, 5);
    }

    #[test]
    fn test_input_assembler_sets_placeholder_for_empty_input() {
        let model = InputModel::default();
        let vm = InputViewAssembler::assemble_from_model(&model, 0, 0, true);
        assert!(vm.placeholder.is_some());
    }

    #[test]
    fn test_input_assembler_shows_queued_hint() {
        let model = InputModel::default();
        let vm = InputViewAssembler::assemble_from_model(&model, 2, 0, true);
        assert_eq!(vm.queued_hint.as_deref(), Some("已排队 2 条"));
    }
}
