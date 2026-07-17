//! 权限配置

use serde::{Deserialize, Serialize};

/// Permission configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionConfig {
    /// Default permission mode
    #[serde(default)]
    pub mode: PermissionModeConfig,

    /// Auto-approved tools
    #[serde(default)]
    pub auto_approve: Vec<String>,

    /// Always-deny tools
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Permission mode configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModeConfig {
    /// Ask for permission on every tool call
    #[default]
    Ask,
    /// Auto-approve read-only tools
    AutoRead,
    /// Auto-approve all tools (dangerous)
    AllowAll,
}
