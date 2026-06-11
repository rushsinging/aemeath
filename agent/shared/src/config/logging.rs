//! 日志配置

use serde::{Deserialize, Serialize};

/// Sub-agent log configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentLogConfig {
    /// Enable sub-agent logging
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Include LLM request payload (messages/system_blocks/tool_schemas)
    #[serde(default = "default_true")]
    pub include_request_payload: bool,
    /// Maximum payload bytes (truncate beyond this)
    #[serde(default = "default_max_payload_bytes")]
    pub max_payload_bytes: usize,
}

fn default_true() -> bool {
    true
}
pub(crate) fn default_max_payload_bytes() -> usize {
    65536
}

impl Default for SubAgentLogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_request_payload: true,
            max_payload_bytes: 65536,
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Global log level for all modules (trace/debug/info/warn/error)
    #[serde(default = "default_level", alias = "default_level")]
    pub level: String,

    /// Maximum log file size in bytes
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,

    /// Maximum rotated backup files
    #[serde(default = "default_max_backups")]
    pub max_backups: usize,

    /// Retention days for rotated logs
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,

    /// Sub-agent log settings
    #[serde(default)]
    pub sub_agent_log: SubAgentLogConfig,

    /// 分化日志存放目录。默认 ~/.aemeath/logs/，不配时回退到 ~/.aemeath/
    #[serde(default)]
    pub logs_dir: Option<String>,

    /// 是否启用 input/output/tool 分化日志
    #[serde(default = "default_true")]
    pub role_logs_enabled: bool,
}

fn default_level() -> String {
    "warn".to_string()
}
pub(crate) fn default_max_bytes() -> u64 {
    10 * 1024 * 1024 // 10 MB
}
pub(crate) fn default_max_backups() -> usize {
    5
}
pub(crate) fn default_retention_days() -> u64 {
    30
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "warn".to_string(),
            max_bytes: 10 * 1024 * 1024,
            max_backups: 5,
            retention_days: 30,
            sub_agent_log: SubAgentLogConfig::default(),
            logs_dir: None,
            role_logs_enabled: true,
        }
    }
}

impl LoggingConfig {
    /// Build an env_logger filter string from this config.
    pub fn to_filter_string(&self) -> String {
        self.level.clone()
    }

    /// 解析 `level` 字符串为 `log::LevelFilter`，解析失败时回退到 `Warn`。
    pub fn to_level_filter(&self) -> log::LevelFilter {
        logging::level_filter_from_str(&self.level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_config_default_filter_is_global_level() {
        let cfg = LoggingConfig::default();

        assert_eq!(cfg.to_filter_string(), "warn");
    }

    #[test]
    fn test_logging_config_to_filter_string_uses_global_level() {
        let cfg = LoggingConfig {
            level: "info".to_string(),
            ..Default::default()
        };

        assert_eq!(cfg.to_filter_string(), "info");
    }

    #[test]
    fn test_logging_config_deserializes_legacy_default_level_alias() {
        let cfg: LoggingConfig = serde_json::from_str(r#"{"default_level":"debug"}"#)
            .expect("legacy default_level should deserialize");

        assert_eq!(cfg.to_filter_string(), "debug");
    }

    #[test]
    fn test_sub_agent_log_default() {
        let cfg = SubAgentLogConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.include_request_payload);
        assert_eq!(cfg.max_payload_bytes, 65536);
    }
}
