use crate::tui::view_state::AppViewState;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MouseEventMapping {
    pub output_scroll_delta: i16,
}

pub fn map_mouse_scroll(delta: i16) -> MouseEventMapping {
    MouseEventMapping {
        output_scroll_delta: delta,
    }
}

pub fn apply_mouse_mapping(view_state: &mut AppViewState, mapping: MouseEventMapping) {
    if mapping.output_scroll_delta < 0 {
        view_state.output.scroll_offset = view_state
            .output
            .scroll_offset
            .saturating_add(mapping.output_scroll_delta.unsigned_abs() as usize);
    } else {
        view_state.output.scroll_offset = view_state
            .output
            .scroll_offset
            .saturating_sub(mapping.output_scroll_delta as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_mouse_scroll_records_delta() {
        assert_eq!(map_mouse_scroll(-1).output_scroll_delta, -1);
    }

    #[test]
    fn test_apply_mouse_mapping_scrolls_down() {
        let mut view_state = AppViewState::default();
        apply_mouse_mapping(&mut view_state, map_mouse_scroll(-2));
        assert_eq!(view_state.output.scroll_offset, 2);
    }

    #[test]
    fn test_apply_mouse_mapping_scrolls_up_saturating() {
        let mut view_state = AppViewState::default();
        apply_mouse_mapping(&mut view_state, map_mouse_scroll(2));
        assert_eq!(view_state.output.scroll_offset, 0);
    }
}
