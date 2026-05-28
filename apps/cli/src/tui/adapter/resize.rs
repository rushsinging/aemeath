use crate::tui::view_state::AppViewState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeMapping {
    pub width: u16,
    pub height: u16,
}

pub fn map_resize(width: u16, height: u16) -> ResizeMapping {
    ResizeMapping { width, height }
}

pub fn apply_resize(view_state: &mut AppViewState, mapping: ResizeMapping) {
    view_state.layout.terminal_width = mapping.width;
    view_state.layout.terminal_height = mapping.height;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_resize_records_width() {
        assert_eq!(map_resize(80, 24).width, 80);
    }

    #[test]
    fn test_map_resize_records_height() {
        assert_eq!(map_resize(80, 24).height, 24);
    }

    #[test]
    fn test_apply_resize_updates_view_state() {
        let mut view_state = AppViewState::default();
        apply_resize(&mut view_state, map_resize(100, 40));
        assert_eq!(view_state.layout.terminal_width, 100);
        assert_eq!(view_state.layout.terminal_height, 40);
    }
}
