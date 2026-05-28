use std::collections::HashSet;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputViewState {
    pub scroll_offset: usize,
    pub follow_tail: bool,
    pub auto_scroll: bool,
    pub is_selecting: bool,
    pub selection_start: Option<SelectedTextRange>,
    pub selection_end: Option<SelectedTextRange>,
    pub selected_text_range: Option<SelectedTextRange>,
    pub screen_line_map: Vec<ScreenLineMapEntry>,
    pub last_visible_height: usize,
    pub render_revision: u64,
    pub collapsed_blocks: HashSet<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedTextRange {
    pub start_block_key: String,
    pub start_offset: usize,
    pub end_block_key: String,
    pub end_offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenLineMapEntry {
    pub block_key: String,
    pub line_index: usize,
}
