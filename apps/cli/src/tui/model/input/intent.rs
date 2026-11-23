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
    /// 当前逻辑行行首（不跨行）。
    MoveCursorLineHome,
    /// 当前逻辑行行尾（不跨行）。
    MoveCursorLineEnd,
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
