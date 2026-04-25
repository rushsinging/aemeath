//! 存储配置

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) fn default_max_sessions() -> usize {
    100
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Directory for session storage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions_dir: Option<PathBuf>,

    /// Enable session persistence
    #[serde(default = "super::ui::default_true")]
    pub persist_sessions: bool,

    /// Maximum sessions to keep
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,

    /// Enable history
    #[serde(default = "super::ui::default_true")]
    pub history: bool,

    /// History file path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_file: Option<PathBuf>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            sessions_dir: None,
            persist_sessions: true,
            max_sessions: default_max_sessions(),
            history: true,
            history_file: None,
        }
    }
}
