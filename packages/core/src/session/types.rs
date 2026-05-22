//! 会话核心类型定义

use crate::config::paths;
use crate::message::{Message, Role};
use crate::state;
use crate::task::TaskSnapshot;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Validate a session ID — delegates to state::validate_session_id
pub fn validate_session_id(id: &str) -> Result<(), String> {
    state::validate_session_id(id)
}

/// Session metadata for organizing and filtering sessions
#[derive(Serialize, Deserialize, Default, Clone)]
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

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub updated_at: String,
    /// Session metadata for organization
    #[serde(default)]
    pub metadata: SessionMetadata,
    /// Task snapshot for resuming in-progress tasks
    #[serde(default)]
    pub tasks: Option<TaskSnapshot>,
}

impl Session {
    /// Create a new session with default metadata
    pub fn new(id: String, cwd: String) -> Self {
        let project = extract_project_name(&cwd);
        Self {
            id,
            cwd,
            messages: Vec::new(),
            created_at: now_iso(),
            updated_at: now_iso(),
            metadata: SessionMetadata {
                project,
                ..Default::default()
            },
            tasks: None,
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
        let first_user = self.messages.iter().find(|m| m.role == Role::User);
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

/// Get the sessions directory (`~/.agents/sessions/`)
pub fn sessions_dir() -> PathBuf {
    paths::global_sessions_dir()
}

/// Generate a new session ID — delegates to state::new_session_id for consistency
pub fn new_session_id() -> String {
    crate::state::new_session_id()
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
