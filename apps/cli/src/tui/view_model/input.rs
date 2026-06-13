#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputAreaViewModel {
    pub text: String,
    pub cursor: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub placeholder: Option<String>,
    pub mode_label: Option<String>,
    pub queued_hint: Option<String>,
    pub disabled_reason: Option<String>,
    pub pending_images: usize,
    pub focused: bool,
}

impl InputAreaViewModel {
    pub fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }
}
