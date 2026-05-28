use crate::tui::view_model::InputAreaViewModel;

pub struct InputViewAssembler;

impl InputViewAssembler {
    pub fn assemble_text(text: &str, cursor: usize) -> InputAreaViewModel {
        InputAreaViewModel {
            text: text.to_string(),
            cursor,
            placeholder: None,
            mode_label: None,
            queued_hint: None,
            disabled_reason: None,
        }
    }
}
