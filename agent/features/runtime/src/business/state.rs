//! Application state management
//!
//! Provides persistent state storage and session management.

pub mod settings;
pub use settings::{PermissionMode, Settings};

use crate::utils::bootstrap::config_paths as paths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use uuid::{NoContext, Timestamp, Uuid};

/// Session ID type
pub type SessionId = String;

/// Generate a new UUIDv7 session ID.
///
/// UUIDv7 is time-ordered, globally unique without local counter state, and remains
/// filename-safe. Existing 24-hex legacy IDs remain valid and loadable.
pub fn new_session_id() -> SessionId {
    new_session_id_with_timestamp(Timestamp::now(NoContext))
}

fn new_session_id_with_timestamp(timestamp: Timestamp) -> SessionId {
    Uuid::new_v7(timestamp).to_string()
}

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

/// A single message in a session
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMessage {
    /// Role: user, assistant, or tool
    pub role: String,
    /// Message content
    pub content: String,
    /// Timestamp (milliseconds since epoch)
    pub timestamp: u64,
    /// Tool call info if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Tool result if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<String>,
}

/// A saved session (internal use only, distinct from session::Session)
/// This is used by AppState for runtime session management.
/// For CLI persistence, use crate::business::session::Session instead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InternalSession {
    /// Unique session ID
    pub id: SessionId,
    /// Working directory at session start
    pub cwd: String,
    /// Creation timestamp
    pub created_at: u64,
    /// Last updated timestamp
    pub updated_at: u64,
    /// Messages in the session
    pub messages: Vec<SessionMessage>,
    /// Token usage (if tracked)
    #[serde(default)]
    pub total_tokens: u64,
}

/// Alias for backward compatibility during transition
#[deprecated(
    since = "0.1.0",
    note = "Use InternalSession or crate::business::session::Session instead"
)]
pub type Session = InternalSession;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl InternalSession {
    /// Create a new session
    pub fn new(cwd: &Path) -> Self {
        let now = now_ms();
        Self {
            id: new_session_id(),
            cwd: cwd.to_string_lossy().to_string(),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            total_tokens: 0,
        }
    }

    /// Add a message to the session
    pub fn add_message(&mut self, role: &str, content: &str) {
        self.updated_at = now_ms();
        self.messages.push(SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: self.updated_at,
            tool_name: None,
            tool_result: None,
        });
    }

    /// Add a tool call message
    pub fn add_tool_call(&mut self, tool_name: &str, input: &str) {
        self.updated_at = now_ms();
        self.messages.push(SessionMessage {
            role: "tool".to_string(),
            content: input.to_string(),
            timestamp: self.updated_at,
            tool_name: Some(tool_name.to_string()),
            tool_result: None,
        });
    }

    /// Add a tool result
    pub fn add_tool_result(&mut self, tool_name: &str, result: &str, is_error: bool) {
        self.updated_at = now_ms();
        self.messages.push(SessionMessage {
            role: "tool_result".to_string(),
            content: result.to_string(),
            timestamp: self.updated_at,
            tool_name: Some(tool_name.to_string()),
            tool_result: if is_error {
                Some("error".to_string())
            } else {
                None
            },
        });
    }
}

/// Application state manager
pub struct AppState {
    /// Settings
    settings: RwLock<Settings>,
    /// Active sessions (session_id -> InternalSession)
    sessions: RwLock<HashMap<SessionId, InternalSession>>,
    /// Settings file path
    settings_path: PathBuf,
    /// Sessions directory
    sessions_dir: PathBuf,
}

impl AppState {
    /// Create a new state manager
    pub fn new() -> Self {
        Self {
            settings: RwLock::new(Settings::default()),
            sessions: RwLock::new(HashMap::new()),
            settings_path: paths::global_settings_path(),
            sessions_dir: paths::global_sessions_dir(),
        }
    }

    /// Load settings from disk
    pub async fn load_settings(&self) -> Result<(), String> {
        if !self.settings_path.exists() {
            return Ok(());
        }
        let content = tokio::fs::read_to_string(&self.settings_path)
            .await
            .map_err(|e| format!("Failed to read settings: {e}"))?;
        let settings: Settings =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {e}"))?;
        *self.settings.write().await = settings;
        Ok(())
    }

    /// Save settings to disk
    pub async fn save_settings(&self) -> Result<(), String> {
        if let Some(parent) = self.settings_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }
        let settings = self.settings.read().await.clone();
        let content = serde_json::to_string_pretty(&settings)
            .map_err(|e| format!("Failed to serialize settings: {e}"))?;
        tokio::fs::write(&self.settings_path, content)
            .await
            .map_err(|e| format!("Failed to write settings: {e}"))?;
        Ok(())
    }

    /// Get current settings
    pub async fn get_settings(&self) -> Settings {
        self.settings.read().await.clone()
    }

    /// Update settings
    pub async fn update_settings<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Settings),
    {
        let mut settings = self.settings.write().await;
        f(&mut settings);
        drop(settings);
        self.save_settings().await
    }

    /// Get or create a session
    pub async fn get_or_create_session(
        &self,
        session_id: Option<&str>,
        cwd: &Path,
    ) -> InternalSession {
        if let Some(id) = session_id {
            if let Some(session) = self.load_session(id).await {
                return session;
            }
        }
        let session = InternalSession::new(cwd);
        if let Err(e) = self.save_session(&session).await {
            log::warn!("failed to persist new session {}: {e}", session.id);
        }
        session
    }

    /// Load a session from disk
    pub async fn load_session(&self, session_id: &str) -> Option<InternalSession> {
        validate_session_id(session_id).ok()?;
        // Use write lock directly to avoid TOCTOU race between read and write
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get(session_id) {
            return Some(session.clone());
        }
        let session_path = self.sessions_dir.join(format!("{session_id}.json"));
        if !session_path.exists() {
            return None;
        }
        let content = tokio::fs::read_to_string(&session_path).await.ok()?;
        let session: InternalSession = serde_json::from_str(&content).ok()?;
        sessions.insert(session_id.to_string(), session.clone());
        Some(session)
    }

    /// Save a session to disk
    pub async fn save_session(&self, session: &InternalSession) -> Result<(), String> {
        validate_session_id(&session.id)?;
        tokio::fs::create_dir_all(&self.sessions_dir)
            .await
            .map_err(|e| format!("Failed to create sessions directory: {e}"))?;
        let session_path = self.sessions_dir.join(format!("{}.json", session.id));
        let content = serde_json::to_string_pretty(session)
            .map_err(|e| format!("Failed to serialize session: {e}"))?;
        tokio::fs::write(&session_path, content)
            .await
            .map_err(|e| format!("Failed to write session: {e}"))?;
        self.sessions
            .write()
            .await
            .insert(session.id.clone(), session.clone());
        Ok(())
    }

    /// List recent sessions
    pub async fn list_sessions(&self) -> Result<Vec<InternalSession>, String> {
        if !self.sessions_dir.exists() {
            return Ok(Vec::new());
        }
        let mut entries = tokio::fs::read_dir(&self.sessions_dir)
            .await
            .map_err(|e| format!("Failed to read sessions directory: {e}"))?;
        let mut sessions = Vec::new();
        let mem_cache = self.sessions.read().await;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if let Some(cached) = mem_cache.get(file_stem) {
                    sessions.push(cached.clone());
                    continue;
                }
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(session) = serde_json::from_str::<InternalSession>(&content) {
                        sessions.push(session);
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        validate_session_id(session_id)?;
        self.sessions.write().await.remove(session_id);
        let session_path = self.sessions_dir.join(format!("{session_id}.json"));
        if session_path.exists() {
            tokio::fs::remove_file(&session_path)
                .await
                .map_err(|e| format!("Failed to delete session: {e}"))?;
        }
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::{NoContext, Timestamp, Uuid};

    #[test]
    fn test_session_creation() {
        let session = InternalSession::new(Path::new("/tmp"));
        assert!(!session.id.is_empty());
        assert_eq!(Uuid::parse_str(&session.id).unwrap().get_version_num(), 7);
        assert_eq!(session.cwd, "/tmp");
    }

    #[test]
    fn test_new_session_id_happy_path_is_uuidv7() {
        let id = new_session_id();
        let uuid = Uuid::parse_str(&id).unwrap();

        assert_eq!(id.len(), 36);
        assert_eq!(uuid.get_version_num(), 7);
        assert!(validate_session_id(&id).is_ok());
    }

    #[test]
    fn test_new_session_id_boundary_same_timestamp_still_unique() {
        let timestamp = Timestamp::from_unix(NoContext, 1_700_000_000, 123_000_000);

        let first = new_session_id_with_timestamp(timestamp);
        let second = new_session_id_with_timestamp(timestamp);

        assert_ne!(first, second);
        assert_eq!(Uuid::parse_str(&first).unwrap().get_version_num(), 7);
        assert_eq!(Uuid::parse_str(&second).unwrap().get_version_num(), 7);
    }

    #[test]
    fn test_new_session_id_error_path_rejects_malformed_uuid_like_id() {
        let malformed = "018f2d4e-9c7a-7b12-9a34-8f0c1d2e3f45/evil";

        assert!(validate_session_id(malformed).is_err());
    }

    #[test]
    fn test_validate_session_id_accepts_legacy_hex_id() {
        assert!(validate_session_id("0000019dc93bab86dfd7032f").is_ok());
    }
}
