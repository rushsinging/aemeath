//! Session-owned value types and identifiers.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::{NoContext, Timestamp, Uuid};

/// Validate a session ID to prevent path traversal attacks.
pub fn validate_session_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("session ID must not be empty".to_string());
    }
    if !id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err(format!(
            "invalid session ID: {id:?} — only alphanumeric characters, hyphens, and underscores are allowed"
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub is_favorite: bool,
    pub model: Option<String>,
    pub project: Option<String>,
}

pub use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

pub fn extract_project_name(cwd: &str) -> Option<String> {
    PathBuf::from(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

pub fn new_session_id() -> String {
    Uuid::new_v7(Timestamp::now(NoContext)).to_string()
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
