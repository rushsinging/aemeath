use super::submission::InputSubmission;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputChange {
    TextChanged { text: String, cursor: usize },
    CursorMoved { cursor: usize },
    Submitted { submission: InputSubmission },
    Cleared,
}
