#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertText(String),
    MoveCursor(usize),
    DeleteBackward,
    Submit,
    Clear,
}
