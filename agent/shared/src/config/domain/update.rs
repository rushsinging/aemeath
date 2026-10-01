//! 更新检查配置。

use serde::{Deserialize, Serialize};

/// 更新配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// 是否在启动时检查更新（默认 true）。
    #[serde(default = "default_check_on_startup")]
    pub check_on_startup: bool,

    /// 更新渠道：`"stable"` 仅正式 release，`"prerelease"` 含 pre-release。
    #[serde(default = "default_channel")]
    pub channel: String,
}

fn default_check_on_startup() -> bool {
    true
}

fn default_channel() -> String {
    "stable".into()
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_startup: default_check_on_startup(),
            channel: default_channel(),
        }
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
