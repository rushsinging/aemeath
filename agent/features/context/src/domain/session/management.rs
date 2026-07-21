use serde::{Deserialize, Serialize};
use share::message::{Message, Role};

use super::{CanonicalSession, SessionMetadata};

/// Context-owned session list projection published to Runtime/SDK adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionListEntry {
    pub id: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
    pub preview: Option<String>,
    pub summary: String,
}

impl SessionListEntry {
    pub(crate) fn from_canonical(session: &CanonicalSession) -> Self {
        let messages = session.structured_messages();
        let preview = messages
            .iter()
            .find(|message| message.role == Role::User)
            .and_then(first_line);
        let summary = session
            .metadata
            .title
            .clone()
            .or_else(|| preview.clone())
            .or_else(|| session.metadata.project.clone())
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            id: session.id.clone(),
            title: session.metadata.title.clone(),
            project: session.metadata.project.clone(),
            model: session.metadata.model.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            message_count: messages.len(),
            preview,
            summary,
        }
    }
}

fn first_line(message: &Message) -> Option<String> {
    let text = message.text_content();
    let first = text.lines().next().unwrap_or_default().trim();
    (!first.is_empty()).then(|| first.chars().take(50).collect())
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMetadataUpdate {
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub is_favorite: Option<bool>,
}

impl SessionMetadataUpdate {
    pub(crate) fn apply(self, metadata: &mut SessionMetadata) {
        if let Some(title) = self.title {
            metadata.title = Some(title);
        }
        if let Some(tags) = self.tags {
            metadata.tags = tags;
        }
        if let Some(notes) = self.notes {
            metadata.notes = Some(notes);
        }
        if let Some(is_favorite) = self.is_favorite {
            metadata.is_favorite = is_favorite;
        }
    }
}

/// Runtime-safe resume projection. Session internals never cross the crate boundary.
#[derive(Debug, Clone)]
pub struct SessionResumeProjection {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub trimmed: usize,
    pub repaired: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionManagementError {
    #[error("Session 不存在：{0}")]
    NotFound(String),
    #[error("Session 数据损坏：{0}")]
    Corrupt(String),
    #[error("Session schema 版本过新：{0}")]
    UnsupportedFutureVersion(u32),
    #[error("Session 存储失败：{0}")]
    Storage(String),
    #[error("Session 恢复失败：{0}")]
    Resume(String),
}
