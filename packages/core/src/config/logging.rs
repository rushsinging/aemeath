//! 日志配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Default log level for all modules (trace/debug/info/warn/error)
    #[serde(default = "default_level")]
    pub default_level: String,

    /// Per-module log level overrides. Key = module prefix (e.g. "aemeath_llm"), value = level.
    /// Uses env_logger filter syntax: module prefixes are matched against log targets.
    #[serde(default)]
    pub module_levels: HashMap<String, String>,

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
            default_level: "warn".to_string(),
            module_levels: {
                let mut m = HashMap::new();
                m.insert("aemeath_llm".to_string(), "debug".to_string());
                m.insert("aemeath_cli".to_string(), "debug".to_string());
                m.insert("aemeath_core".to_string(), "debug".to_string());
                m.insert("aemeath_tools".to_string(), "debug".to_string());
                m
            },
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
        let mut parts = vec![self.default_level.clone()];
        for (module, level) in &self.module_levels {
            parts.push(format!("{}={}", module, level));
        }
        parts.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_config_default_filter() {
        let cfg = LoggingConfig::default();
        let filter = cfg.to_filter_string();
        assert!(filter.starts_with("warn"));
        assert!(filter.contains("aemeath_llm=debug"));
        assert!(filter.contains("aemeath_tools=debug"));
    }

    #[test]
    fn test_logging_config_to_filter_string_empty_module_levels() {
        let cfg = LoggingConfig {
            default_level: "info".to_string(),
            module_levels: HashMap::new(),
            ..Default::default()
        };
        assert_eq!(cfg.to_filter_string(), "info");
    }

    #[test]
    fn test_logging_config_to_filter_string_custom_levels() {
        let mut levels = HashMap::new();
        levels.insert("my_crate".to_string(), "trace".to_string());
        let cfg = LoggingConfig {
            default_level: "error".to_string(),
            module_levels: levels,
            ..Default::default()
        };
        let filter = cfg.to_filter_string();
        assert!(filter.starts_with("error"));
        assert!(filter.contains("my_crate=trace"));
    }

    #[test]
    fn test_sub_agent_log_default() {
        let cfg = SubAgentLogConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.include_request_payload);
        assert_eq!(cfg.max_payload_bytes, 65536);
    }
}
