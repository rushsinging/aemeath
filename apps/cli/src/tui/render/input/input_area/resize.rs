use super::constants::{INPUT_AREA_MAX_HEIGHT, INPUT_AREA_MIN_HEIGHT};
use super::InputArea;
use crate::tui::render::input::input_area::wrap::wrap_input_lines_for_width;
use crate::tui::view_model::InputAreaViewModel;

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
#[path = "resize_tests.rs"]
mod tests;
