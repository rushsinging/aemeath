//! 会话核心类型定义

use chrono::Utc;
use serde::{Deserialize, Serialize};
use share::message::{Message, Role};
use std::path::PathBuf;
use storage::TaskSnapshot;
use uuid::NoContext;
use uuid::Timestamp;
use uuid::Uuid;

use super::chat_chain::ChatSegment;

/// Validate a session ID to prevent path traversal attacks.
/// Only allows alphanumeric characters, hyphens, and underscores.
pub fn validate_session_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("session ID must not be empty".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "invalid session ID: {id:?} — only alphanumeric characters, hyphens, and underscores are allowed"
        ));
    }
    Ok(())
}

/// Session metadata for organizing and filtering sessions
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct SessionMetadata {
    /// User-defined title for the session
    pub title: Option<String>,
    /// Tags for categorizing sessions
    pub tags: Vec<String>,
    /// Notes or description
    pub notes: Option<String>,
    /// Whether this is a favorite/pinned session
    pub is_favorite: bool,
    /// Model used in this session
    pub model: Option<String>,
    /// Project name (derived from cwd)
    pub project: Option<String>,
}

pub use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    /// 旧格式兼容字段（加载后迁移到 `chats`）。新 session 不写入此字段。
    #[serde(default, skip_serializing)]
    pub messages: Vec<Message>,
    /// Chat 链：按 user 消息分段的对话历史（新格式）。
    ///
    /// compact 产生新链时，旧链保留在此数组（冻结），新 `Compact` 段（`parent_id=None`）
    /// 作为新链起点。resume 只加载活跃链（最后一个 Compact 段到末端）。
    #[serde(default)]
    pub chats: Vec<ChatSegment>,
    pub created_at: String,
    pub updated_at: String,
    /// Session metadata for organization
    #[serde(default)]
    pub metadata: SessionMetadata,
    /// Task snapshot for resuming in-progress tasks
    #[serde(default)]
    pub tasks: Option<TaskSnapshot>,
    #[serde(default)]
    pub workspace: Option<PersistedWorkspaceContext>,
}

impl Session {
    /// Create a new session with default metadata
    pub fn new(id: String, cwd: String) -> Self {
        let project = extract_project_name(&cwd);
        Self {
            id,
            cwd,
            messages: Vec::new(),
            chats: Vec::new(),
            created_at: now_iso(),
            updated_at: now_iso(),
            metadata: SessionMetadata {
                project,
                ..Default::default()
            },
            tasks: None,
            workspace: None,
        }
    }

    /// Set the session title
    pub fn set_title(&mut self, title: String) {
        self.metadata.title = Some(title);
        self.updated_at = now_iso();
    }

    /// Add a tag to the session
    pub fn add_tag(&mut self, tag: String) {
        if !self.metadata.tags.contains(&tag) {
            self.metadata.tags.push(tag);
            self.updated_at = now_iso();
        }
    }

    /// Remove a tag from the session
    pub fn remove_tag(&mut self, tag: &str) {
        self.metadata.tags.retain(|t| t != tag);
        self.updated_at = now_iso();
    }

    /// Set session as favorite
    pub fn set_favorite(&mut self, is_favorite: bool) {
        self.metadata.is_favorite = is_favorite;
        self.updated_at = now_iso();
    }

    /// Set notes/description
    pub fn set_notes(&mut self, notes: String) {
        self.metadata.notes = Some(notes);
        self.updated_at = now_iso();
    }

    /// Set the model used
    pub fn set_model(&mut self, model: String) {
        self.metadata.model = Some(model);
        self.updated_at = now_iso();
    }

    /// Get display title (uses title if set, otherwise generates from cwd/id)
    pub fn display_title(&self) -> String {
        if let Some(title) = &self.metadata.title {
            title.clone()
        } else if let Some(project) = &self.metadata.project {
            format!("{} - {}", project, self.id)
        } else {
            format!("Session {}", self.id)
        }
    }

    /// Build a one-line content summary for display in session lists.
    ///
    /// Priority:
    ///   1. User-set title (truncated to 40 chars)
    ///   2. First user message content (truncated to 50 chars)
    ///   3. Project name fallback
    pub fn summary(&self) -> String {
        if let Some(title) = &self.metadata.title {
            return title.chars().take(40).collect();
        }
        // 优先从 chats 链查找，回退到旧 messages（未迁移的旧 session）
        let first_user = self
            .chats
            .iter()
            .flat_map(|s| s.messages.iter())
            .chain(self.messages.iter())
            .find(|m| m.role == Role::User);
        if let Some(msg) = first_user {
            let text = msg.text_content();
            let first_line = text.lines().next().unwrap_or("").trim();
            let trunc: String = first_line.chars().take(50).collect();
            if !trunc.is_empty() {
                return trunc;
            }
        }
        self.metadata
            .project
            .as_deref()
            .unwrap_or("unknown")
            .to_string()
    }
}

/// Extract project name from cwd path
pub fn extract_project_name(cwd: &str) -> Option<String> {
    let path = PathBuf::from(cwd);
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

/// Generate a new session ID (UUIDv7, time-ordered, filename-safe).
pub fn new_session_id() -> String {
    Uuid::new_v7(Timestamp::now(NoContext)).to_string()
}

/// Get current ISO timestamp
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Filter criteria for session search
#[derive(Default)]
pub struct SessionFilter {
    /// Filter by title (partial match)
    pub title: Option<String>,
    /// Filter by tag (exact match)
    pub tag: Option<String>,
    /// Filter by project (partial match)
    pub project: Option<String>,
    /// Filter by favorite status
    pub is_favorite: Option<bool>,
    /// Filter by model
    pub model: Option<String>,
}
