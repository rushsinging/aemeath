//! Settings and permission-mode type definitions
//!
//! Factored out of [`crate::state`] to keep the parent module focused on
//! session / app-state management.

use serde::{Deserialize, Serialize};

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
    AllowAll,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.model, "claude-sonnet-4-6");
        assert_eq!(settings.max_tokens, 200000);
        assert_eq!(settings.permission_mode, PermissionMode::Ask);
    }
}
