//! UI 配置

use serde::{Deserialize, Serialize};

pub(crate) fn default_true() -> bool {
    true
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Enable markdown rendering
    #[serde(default = "default_true")]
    pub markdown: bool,

    /// Enable syntax highlighting
    #[serde(default = "default_true")]
    pub syntax_highlight: bool,

    /// Show progress indicators
    #[serde(default = "default_true")]
    pub progress: bool,

    /// Color output
    #[serde(default = "default_true")]
    pub color: bool,

    /// Verbose output
    #[serde(default)]
    pub verbose: bool,

    /// TUI mode
    #[serde(default = "default_true")]
    pub tui: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            markdown: true,
            syntax_highlight: true,
            progress: true,
            color: true,
            verbose: false,
            tui: true,
        }
    }
}
