use crate::message::Message;
use crate::state;
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
        let first_user = self.messages.iter().find(|m| m.role == crate::message::Role::User);
        if let Some(msg) = first_user {
            let text = msg.text_content();
            let first_line = text.lines().next().unwrap_or("").trim();
            let trunc: String = first_line.chars().take(50).collect();
            if !trunc.is_empty() {
                return trunc;
            }
        }
        self.metadata.project.as_deref().unwrap_or("unknown").to_string()
    }
}

/// Extract project name from cwd path
fn extract_project_name(cwd: &str) -> Option<String> {
    let path = PathBuf::from(cwd);
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

/// Get the sessions directory (~/.aemeath/sessions/)
pub fn sessions_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".aemeath").join("sessions")
}

/// Save a session to disk
pub async fn save_session(session: &Session) -> Result<(), String> {
    validate_session_id(&session.id)?;

    let dir = sessions_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("failed to create sessions dir: {e}"))?;

    let path = dir.join(format!("{}.json", session.id));
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| format!("failed to serialize session: {e}"))?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| format!("failed to write session: {e}"))?;
    Ok(())
}

/// Load a session from disk by ID
pub async fn load_session(id: &str) -> Result<Session, String> {
    validate_session_id(id)?;

    let path = sessions_dir().join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("session not found: {id}"));
    }
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("failed to read session: {e}"))?;
    serde_json::from_str(&json).map_err(|e| format!("failed to parse session: {e}"))
}

/// List all saved sessions, sorted by updated_at descending
pub async fn list_sessions() -> Vec<Session> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(json) = tokio::fs::read_to_string(&path).await {
                if let Ok(session) = serde_json::from_str::<Session>(&json) {
                    sessions.push(session);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
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

/// Search sessions with filter criteria
pub async fn search_sessions(filter: &SessionFilter) -> Vec<Session> {
    let sessions = list_sessions().await;

    sessions
        .into_iter()
        .filter(|s| {
            // Title filter (partial match)
            if let Some(title) = &filter.title {
                let matches = s.metadata.title.as_ref()
                    .map(|t| t.to_lowercase().contains(&title.to_lowercase()))
                    .unwrap_or(false);
                if !matches {
                    return false;
                }
            }

            // Tag filter (exact match)
            if let Some(tag) = &filter.tag {
                if !s.metadata.tags.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
                    return false;
                }
            }

            // Project filter (partial match)
            if let Some(project) = &filter.project {
                let matches = s.metadata.project.as_ref()
                    .map(|p| p.to_lowercase().contains(&project.to_lowercase()))
                    .unwrap_or(false);
                if !matches {
                    return false;
                }
            }

            // Favorite filter
            if let Some(is_favorite) = filter.is_favorite {
                if s.metadata.is_favorite != is_favorite {
                    return false;
                }
            }

            // Model filter (exact match)
            if let Some(model) = &filter.model {
                if s.metadata.model.as_ref().map(|m| m != model).unwrap_or(true) {
                    return false;
                }
            }

            true
        })
        .collect()
}

/// Delete a session by ID
pub async fn delete_session(id: &str) -> Result<(), String> {
    validate_session_id(id)?;

    let path = sessions_dir().join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("session not found: {id}"));
    }
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("failed to delete session: {e}"))
}

/// Update session metadata
pub async fn update_session_metadata(
    id: &str,
    title: Option<String>,
    tags: Option<Vec<String>>,
    notes: Option<String>,
    is_favorite: Option<bool>,
) -> Result<Session, String> {
    let mut session = load_session(id).await?;

    if let Some(t) = title {
        session.set_title(t);
    }
    if let Some(new_tags) = tags {
        session.metadata.tags = new_tags;
        session.updated_at = now_iso();
    }
    if let Some(n) = notes {
        session.set_notes(n);
    }
    if let Some(fav) = is_favorite {
        session.set_favorite(fav);
    }

    save_session(&session).await?;
    Ok(session)
}

/// Generate a new session ID — delegates to state::new_session_id for consistency
pub fn new_session_id() -> String {
    crate::state::new_session_id()
}

/// Get current ISO timestamp
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
