#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputAreaViewModel {
    pub text: String,
    pub cursor: usize,
    pub placeholder: Option<String>,
    pub mode_label: Option<String>,
    pub queued_hint: Option<String>,
    pub disabled_reason: Option<String>,
}
