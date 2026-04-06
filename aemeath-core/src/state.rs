//! Application state management
//!
//! Provides persistent state storage and session management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// Session ID type
pub type SessionId = String;

/// Generate a new unique session ID
pub fn new_session_id() -> SessionId {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let random: u32 = rand::random();
    format!("{:016x}{:08x}", timestamp, random)
}

/// Application settings that persist across sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// API key (stored encrypted in the future)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Base URL for API
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Model to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Max tokens for responses
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Context window size
    #[serde(default = "default_context_size")]
    pub context_size: usize,

    /// Permission mode
    #[serde(default)]
    pub permission_mode: PermissionMode,

    /// Auto-approve tools (by name)
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,

    /// Denied tools (by name)
    #[serde(default)]
    pub deny_tools: Vec<String>,

    /// Custom system prompt additions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,

    /// Enable markdown rendering
    #[serde(default = "default_true")]
    pub markdown: bool,

    /// Verbose output
    #[serde(default)]
    pub verbose: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            model: default_model(),
            max_tokens: default_max_tokens(),
            context_size: default_context_size(),
            permission_mode: PermissionMode::default(),
            auto_approve_tools: Vec::new(),
            deny_tools: Vec::new(),
            custom_prompt: None,
            markdown: true,
            verbose: false,
        }
    }
}

fn default_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_max_tokens() -> u32 {
    200000
}

fn default_context_size() -> usize {
    128000
}

fn default_true() -> bool {
    true
}

/// Permission modes for tool execution
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Ask for permission on every tool call
    #[default]
    Ask,
    /// Auto-approve read-only tools
    AutoRead,
    /// Auto-approve all tools (dangerous)
    AutoAll,
}

/// A single message in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// For CLI persistence, use crate::session::Session instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[deprecated(since = "0.1.0", note = "Use InternalSession or crate::session::Session instead")]
pub type Session = InternalSession;

impl InternalSession {
    /// Create a new session
    pub fn new(cwd: &Path) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.updated_at = now;
        self.messages.push(SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: now,
            tool_name: None,
            tool_result: None,
        });
    }

    /// Add a tool call message
    pub fn add_tool_call(&mut self, tool_name: &str, input: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.updated_at = now;
        self.messages.push(SessionMessage {
            role: "tool".to_string(),
            content: input.to_string(),
            timestamp: now,
            tool_name: Some(tool_name.to_string()),
            tool_result: None,
        });
    }

    /// Add a tool result
    pub fn add_tool_result(&mut self, tool_name: &str, result: &str, is_error: bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.updated_at = now;
        self.messages.push(SessionMessage {
            role: "tool_result".to_string(),
            content: result.to_string(),
            timestamp: now,
            tool_name: Some(tool_name.to_string()),
            tool_result: if is_error { Some("error".to_string()) } else { None },
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
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_dir = home.join(".config").join("aemeath");
        let sessions_dir = config_dir.join("sessions");

        Self {
            settings: RwLock::new(Settings::default()),
            sessions: RwLock::new(HashMap::new()),
            settings_path: config_dir.join("settings.json"),
            sessions_dir,
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

        let settings: Settings = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings: {e}"))?;

        *self.settings.write().await = settings;
        Ok(())
    }

    /// Save settings to disk
    pub async fn save_settings(&self) -> Result<(), String> {
        // Ensure parent directory exists
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
    pub async fn get_or_create_session(&self, session_id: Option<&str>, cwd: &Path) -> InternalSession {
        if let Some(id) = session_id {
            // Try to load existing session
            if let Some(session) = self.load_session(id).await {
                return session;
            }
        }

        // Create new session
        let session = InternalSession::new(cwd);
        session
    }

    /// Load a session from disk
    pub async fn load_session(&self, session_id: &str) -> Option<InternalSession> {
        // Check in-memory first
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(session_id) {
                return Some(session.clone());
            }
        }

        // Try to load from disk
        let session_path = self.sessions_dir.join(format!("{session_id}.json"));
        if !session_path.exists() {
            return None;
        }

        let content = tokio::fs::read_to_string(&session_path).await.ok()?;
        let session: InternalSession = serde_json::from_str(&content).ok()?;

        // Cache in memory
        self.sessions
            .write()
            .await
            .insert(session_id.to_string(), session.clone());

        Some(session)
    }

    /// Save a session to disk
    pub async fn save_session(&self, session: &InternalSession) -> Result<(), String> {
        // Ensure sessions directory exists
        tokio::fs::create_dir_all(&self.sessions_dir)
            .await
            .map_err(|e| format!("Failed to create sessions directory: {e}"))?;

        // Cache in memory
        self.sessions
            .write()
            .await
            .insert(session.id.clone(), session.clone());

        // Save to disk
        let session_path = self.sessions_dir.join(format!("{}.json", session.id));
        let content = serde_json::to_string_pretty(session)
            .map_err(|e| format!("Failed to serialize session: {e}"))?;

        tokio::fs::write(&session_path, content)
            .await
            .map_err(|e| format!("Failed to write session: {e}"))?;

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
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(session) = serde_json::from_str::<InternalSession>(&content) {
                        sessions.push(session);
                    }
                }
            }
        }

        // Sort by updated_at descending
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(sessions)
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<(), String> {
        // Remove from memory
        self.sessions.write().await.remove(session_id);

        // Remove from disk
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

    #[test]
    fn test_session_creation() {
        let session = Session::new(Path::new("/tmp"));
        assert!(!session.id.is_empty());
        assert_eq!(session.cwd, "/tmp");
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.model, "claude-sonnet-4-6");
        assert_eq!(settings.max_tokens, 200000);
        assert_eq!(settings.permission_mode, PermissionMode::Ask);
    }
}