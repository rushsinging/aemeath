use super::completion_item::CompletionItem;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertChar(char),
    InsertText(String),
    InsertPastedText(String),
    InsertImage(sdk::ClipboardImageView),
    ReplaceText(String),
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorHome,
    MoveCursorEnd,
    InsertNewline,
    DeleteBackward,
    DeleteWordBeforeCursor,
    ReplaceHistory(Vec<String>),
    SetCompletions {
        query: String,
        items: Vec<CompletionItem>,
    },
    SelectCompletionNext,
    SelectCompletionPrevious,
    AcceptCompletion,
    Submit,
    Clear,
}
