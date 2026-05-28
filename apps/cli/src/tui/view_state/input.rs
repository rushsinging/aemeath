#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputViewState {
    pub focused: bool,
    pub viewport_offset: usize,
    pub preferred_column: Option<usize>,
    pub composing: bool,
    pub cursor_blink_visible: bool,
    pub completion_selected_index: Option<usize>,
    pub version: u64,
}
