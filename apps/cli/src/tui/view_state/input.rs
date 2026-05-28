#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputViewState {
    pub cursor_blink_visible: bool,
    pub completion_selected_index: Option<usize>,
    pub version: u64,
}
