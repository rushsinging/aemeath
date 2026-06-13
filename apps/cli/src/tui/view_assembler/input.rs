use crate::tui::model::input::document::InputDocument;
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
        let placeholder = model
            .document
            .buffer
            .is_empty()
            .then(|| "输入消息...".to_string());
        let mut vm = Self::from_document(&model.document, placeholder, pending_images, focused);
        vm.queued_hint = (queued_count > 0).then(|| format!("已排队 {queued_count} 条"));
        vm
    }

    pub fn from_document(
        document: &InputDocument,
        placeholder: Option<String>,
        pending_images: usize,
        focused: bool,
    ) -> InputAreaViewModel {
        let cursor = clamp_to_char_boundary(&document.buffer, document.cursor);
        let (cursor_row, cursor_col) = byte_cursor_to_row_col(&document.buffer, cursor);
        InputAreaViewModel {
            text: document.buffer.clone(),
            cursor,
            cursor_row,
            cursor_col,
            placeholder,
            mode_label: None,
            queued_hint: None,
            disabled_reason: None,
            pending_images,
            focused,
        }
    }
}

fn byte_cursor_to_row_col(text: &str, cursor: usize) -> (usize, usize) {
    let cursor = clamp_to_char_boundary(text, cursor);
    let before_cursor = safe_byte_prefix(text, cursor);
    let row = before_cursor.matches('\n').count();
    let col = before_cursor
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count())
        .unwrap_or_else(|| before_cursor.chars().count());
    (row, col)
}

fn safe_byte_prefix(s: &str, offset: usize) -> &str {
    let mut offset = offset.min(s.len());
    while offset > 0 && !s.is_char_boundary(offset) {
        offset -= 1;
    }
    s.get(..offset).unwrap_or("")
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
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
