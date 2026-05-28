use std::collections::HashSet;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputViewState {
    pub scroll_offset: usize,
    pub follow_tail: bool,
    pub collapsed_blocks: HashSet<String>,
    pub selected_text_range: Option<SelectedTextRange>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedTextRange {
    pub start_block_key: String,
    pub start_offset: usize,
    pub end_block_key: String,
    pub end_offset: usize,
}
