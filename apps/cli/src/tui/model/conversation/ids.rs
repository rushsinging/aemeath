//! Re-export SDK ID types for TUI conversation model.

pub use sdk::ids::{ChatId, ChatTurnId, ToolCallId};

/// Tool stream key for identifying tool call streams.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolStreamKey {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub name: String,
    pub index: usize,
}

impl ToolStreamKey {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId, name: impl Into<String>, index: usize) -> Self {
        Self {
            chat_id,
            turn_id,
            name: name.into(),
            index,
        }
    }
}
