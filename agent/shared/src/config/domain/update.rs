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
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = UpdateConfig::default();
        assert!(config.check_on_startup);
        assert_eq!(config.channel, "stable");
    }

    #[test]
    fn test_deserialize_partial() {
        let json = r#"{"check_on_startup": false}"#;
        let config: UpdateConfig = serde_json::from_str(json).unwrap();
        assert!(!config.check_on_startup);
        assert_eq!(config.channel, "stable"); // 默认值
    }

    #[test]
    fn test_deserialize_empty() {
        let json = r#"{}"#;
        let config: UpdateConfig = serde_json::from_str(json).unwrap();
        assert!(config.check_on_startup); // 默认值
        assert_eq!(config.channel, "stable"); // 默认值
    }
}
