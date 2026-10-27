use share::message::{Message, Role};
use share::session_types::ProjectIdentityData;

use super::{
    extract_project_name, CanonicalSession, DisplayHistoryStepIndexData, SessionSnapshotState,
};

/// 会话列表行（发布语言形态，sdk 同型统一）。
/// 会话列表行投影：canonical → sdk 发布语言（唯一视图模型）。
pub(crate) fn project_list_item(session: &CanonicalSession) -> sdk::SessionItemList {
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
    sdk::SessionItemList {
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

/// project 显示名：显式 `metadata.project` 优先（用户可编辑 override），
/// 否则从 workspace 快照的 project identity 推导目录名。
/// project 的单一事实源是 workspace `ProjectIdentityData`，不在 session
/// 创建时冗余写入 metadata（避免双事实源）。
fn session_display_project(session: &CanonicalSession) -> Option<String> {
    if let Some(explicit) = session.metadata.project.as_deref() {
        return Some(explicit.to_string());
    }
    let SessionSnapshotState::Captured(workspace) = &session.workspace else {
        return None;
    };
    extract_project_name(&workspace.project_identity.initial_cwd)
}

/// Compares stable project identity without treating individual worktree roots
/// as distinct projects. Git projects use `git_common_dir`; non-git projects use
/// their canonical `initial_cwd`.
pub fn session_matches_project(session: &CanonicalSession, current: &ProjectIdentityData) -> bool {
    let SessionSnapshotState::Captured(workspace) = &session.workspace else {
        return false;
    };
    project::same_project_identity(current, &workspace.project_identity)
}

fn first_line(message: &Message) -> Option<String> {
    let text = message.text_content();
    let first = text.lines().next().unwrap_or_default().trim();
    (!first.is_empty()).then(|| first.chars().take(50).collect())
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;

#[derive(Debug, Clone)]
pub struct SessionResumeLoad {
    pub active_session: CanonicalSession,
    pub display_history: Option<DisplayHistoryStepIndexData>,
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
