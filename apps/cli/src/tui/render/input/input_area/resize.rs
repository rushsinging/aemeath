use super::InputArea;
use crate::tui::render::input::input_area::wrap::wrap_input_lines_for_width;
use crate::tui::view_model::InputAreaViewModel;

const INPUT_AREA_MIN_HEIGHT: u16 = 3;
const INPUT_AREA_MAX_HEIGHT: u16 = 8;

impl InputArea {
    pub fn input_content_width(area_width: u16) -> u16 {
        area_width.saturating_sub(2)
    }

    pub fn desired_height(area_width: u16, vm: &InputAreaViewModel) -> u16 {
        let width = Self::input_content_width(area_width) as usize;
        let display_lines = wrap_input_lines_for_width(vm.lines(), width).len() as u16;
        display_lines
            .saturating_add(2)
            .clamp(INPUT_AREA_MIN_HEIGHT, INPUT_AREA_MAX_HEIGHT)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tui::model::input::document::InputDocument;

    fn vm(text: &str) -> InputAreaViewModel {
        let mut document = InputDocument::default();
        document.insert_text(text);
        InputAreaViewModel::from_document(&document, None, 0, true)
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
}
