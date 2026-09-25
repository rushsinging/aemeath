use serde::{Deserialize, Serialize};
use share::message::{Message, Role};
use share::session_types::ProjectIdentity;

use super::{
    extract_project_name, CanonicalSession, DisplayHistoryStepIndex, SessionMetadata,
    SessionRestoreStep, SnapshotState,
};

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
            .unwrap_or_else(|| {
                if messages.is_empty() {
                    "(empty)".to_string()
                } else {
                    "unknown".to_string()
                }
            });
        Self {
            id: session.id.clone(),
            title: session.metadata.title.clone(),
            project: session_display_project(session),
            model: session.metadata.model.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            message_count: messages.len(),
            preview,
            summary,
        }
    }
}

/// project 显示名：显式 `metadata.project` 优先（用户可编辑 override），
/// 否则从 workspace 快照的 project identity 推导目录名。
/// project 的单一事实源是 workspace `ProjectIdentity`，不在 session
/// 创建时冗余写入 metadata（避免双事实源）。
fn session_display_project(session: &CanonicalSession) -> Option<String> {
    if let Some(explicit) = session.metadata.project.as_deref() {
        return Some(explicit.to_string());
    }
    let SnapshotState::Captured(workspace) = &session.workspace else {
        return None;
    };
    extract_project_name(&workspace.project_identity.initial_cwd)
}

/// Compares stable project identity without treating individual worktree roots
/// as distinct projects. Git projects use `git_common_dir`; non-git projects use
/// their canonical `initial_cwd`.
pub fn same_project_identity(current: &ProjectIdentity, persisted: &ProjectIdentity) -> bool {
    match (
        current.git_common_dir.as_deref(),
        persisted.git_common_dir.as_deref(),
    ) {
        (Some(current_git), Some(stored_git)) => current_git == stored_git,
        (None, None) => current.initial_cwd == persisted.initial_cwd,
        _ => false,
    }
}

pub fn session_matches_project(session: &CanonicalSession, current: &ProjectIdentity) -> bool {
    let SnapshotState::Captured(workspace) = &session.workspace else {
        return false;
    };
    same_project_identity(current, &workspace.project_identity)
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

#[derive(Debug, Clone)]
pub struct SessionResumeLoad {
    pub active_session: CanonicalSession,
    pub display_history: Option<DisplayHistoryStepIndex>,
}

/// Runtime-safe resume view. Session internals never cross the crate boundary.
#[derive(Debug, Clone)]
pub struct SessionResumeView {
    pub session_id: String,
    pub active_messages: Vec<Message>,
    pub display_steps: Vec<SessionRestoreStep>,
    pub display_history: Option<DisplayHistoryStepIndex>,
    pub compacted: bool,
    pub created_at: String,
    pub trimmed: usize,
    pub repaired: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionManagementError {
    #[error("Session 不存在：{0}")]
    NotFound(String),
    #[error("Session 不属于当前项目：{0}")]
    ProjectMismatch(String),
    #[error("Session 数据损坏：{0}")]
    Corrupt(String),
    #[error("Session schema 版本过新：{0}")]
    UnsupportedFutureVersion(u32),
    #[error("Session 存储失败：{0}")]
    Storage(String),
    #[error("Session 恢复失败：{0}")]
    Resume(String),
}
