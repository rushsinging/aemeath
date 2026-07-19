//! Canonical Session 恢复投影。

use super::message_integrity::{check_message_integrity, deep_clean_messages, sanitize_messages};
use crate::domain::session::{CanonicalSession, ChatChain};
use share::message::Message;

#[derive(Debug, Clone)]
pub struct SessionRestore {
    pub active_messages: Vec<Message>,
    pub created_at: String,
    pub trimmed: usize,
    pub repaired: usize,
}

impl SessionRestore {
    pub fn from_canonical(session: &CanonicalSession) -> Self {
        let mut messages = ChatChain::from_chats(&session.chats).messages();
        let trimmed = {
            let before = messages.len();
            sanitize_messages(&mut messages);
            before.saturating_sub(messages.len())
        };
        let repaired = if check_message_integrity(&messages).has_issues() {
            deep_clean_messages(&mut messages)
        } else {
            0
        };
        Self {
            active_messages: messages,
            created_at: session.created_at.clone(),
            trimmed,
            repaired,
        }
    }
}
