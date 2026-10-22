//! Session-owned value types and identifiers.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::{NoContext, Timestamp, Uuid};

/// Validate a session ID to prevent path traversal attacks.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub is_favorite: bool,
    pub model: Option<String>,
    /// 会话选定的 reasoning effort 档位（"off".."max"，canonical 取值见
    /// `share::reasoning::ReasoningLevel::as_str`）。`None` 表示未显式选择，
    /// 跟随模型 entry 默认档。
    pub reasoning_effort: Option<String>,
    pub project: Option<String>,
}

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

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
