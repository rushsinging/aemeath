//! Memory 系统配置

use serde::{Deserialize, Serialize};

pub(crate) fn default_max_entries() -> usize {
    100
}

pub(crate) fn default_similarity_threshold() -> f64 {
    0.8
}

pub(crate) fn default_interval_run_steps() -> usize {
    10
}

/// 默认注入条数。自动注入按 pin/确认/新鲜度稳定排序，
/// 与显式 BM25 search 相互独立。
pub(crate) fn default_inject_count() -> usize {
    5
}

pub(crate) fn default_inject_token_budget() -> usize {
    300
}

/// Memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Enable memory system.
    #[serde(default = "super::ui::default_true")]
    pub enabled: bool,

    /// Maximum active entries per layer.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Similarity threshold for deduplication.
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,

    /// Reflection configuration.
    #[serde(default)]
    pub reflection: ReflectionConfig,

    /// 每轮 LLM 调用前注入的稳定优先级 Memory 条目数。
    /// 显式 search 使用 BM25；自动注入不 query-aware。
    #[serde(default = "default_inject_count")]
    pub inject_count: usize,

    /// 自动 Memory 注入的独立估算 token 预算；0 禁用自动注入。
    #[serde(default = "default_inject_token_budget")]
    pub inject_token_budget: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_max_entries(),
            similarity_threshold: default_similarity_threshold(),
            reflection: ReflectionConfig::default(),
            inject_count: default_inject_count(),
            inject_token_budget: default_inject_token_budget(),
        }
    }
}

/// Reflection system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionConfig {
    /// Enable reflection system.
    #[serde(default = "super::ui::default_true")]
    pub enabled: bool,

    /// Trigger reflection every N runs.
    #[serde(default = "default_interval_run_steps")]
    pub interval_run_steps: usize,

    /// Apply suggested memory entries automatically.
    #[serde(default)]
    pub auto_apply_suggestions: bool,

    /// Optional model override for reflection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl Default for ReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_run_steps: default_interval_run_steps(),
            auto_apply_suggestions: false,
            model: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_config_default() {
        let config = MemoryConfig::default();

        assert!(config.enabled);
        assert_eq!(config.max_entries, 100);
        assert_eq!(config.similarity_threshold, 0.8);
        assert!(config.reflection.enabled);
    }

    #[test]
    fn test_memory_config_deserialize_ignores_removed_session_end_summary() {
        let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.enabled);
        assert_eq!(empty.max_entries, 100);
        assert_eq!(empty.reflection.interval_run_steps, 10);
        assert!(!empty.reflection.auto_apply_suggestions);

        let json = r#"{
            "enabled": true,
            "auto_summary_on_session_end": false
        }"#;
        let config: MemoryConfig = serde_json::from_str(json).unwrap();

        assert!(config.enabled);
        assert_eq!(config.max_entries, 100);
    }

    #[test]
    fn test_memory_config_deserialize_custom() {
        let json = r#"{
            "enabled": false,
            "max_entries": 20,
            "similarity_threshold": 0.6,
            "reflection": {
                "enabled": false,
                "interval_run_steps": 5,
                "auto_apply_suggestions": true,
                "model": "test/model"
            }
        }"#;
        let config: MemoryConfig = serde_json::from_str(json).unwrap();

        assert!(!config.enabled);
        assert_eq!(config.max_entries, 20);
        assert_eq!(config.similarity_threshold, 0.6);
        assert!(!config.reflection.enabled);
        assert_eq!(config.reflection.interval_run_steps, 5);
        assert!(config.reflection.auto_apply_suggestions);
        assert_eq!(config.reflection.model.as_deref(), Some("test/model"));
    }

    #[test]
    fn test_reflection_config_default() {
        let config = ReflectionConfig::default();

        assert!(config.enabled);
        assert_eq!(config.interval_run_steps, 10);
        assert!(!config.auto_apply_suggestions);
        assert!(config.model.is_none());
    }
}
