//! Memory 系统配置

use serde::{Deserialize, Serialize};

pub(crate) fn default_max_entries() -> usize {
    100
}

pub(crate) fn default_similarity_threshold() -> f64 {
    0.8
}

pub(crate) fn default_interval_turns() -> usize {
    10
}

/// 默认注入条数。当前按 recency/pin 排序取 top N，相关性不高，
/// 故默认值保守（5 条 ≈ 300 token）。
/// 后续 #551 落地语义检索后应提高此值或改为动态决定。
pub(crate) fn default_inject_count() -> usize {
    5
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

    /// 每轮 LLM 调用前注入 system prompt 的 memory 条目数。
    /// 当前按 recency/pin 排序，非语义相关性——默认值保守。
    /// #551（语义检索）落地后应提高或改为动态。
    #[serde(default = "default_inject_count")]
    pub inject_count: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_max_entries(),
            similarity_threshold: default_similarity_threshold(),
            reflection: ReflectionConfig::default(),
            inject_count: default_inject_count(),
        }
    }
}

/// Reflection system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionConfig {
    /// Enable reflection system.
    #[serde(default = "super::ui::default_true")]
    pub enabled: bool,

    /// Trigger reflection every N turns.
    #[serde(default = "default_interval_turns")]
    pub interval_turns: usize,

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
            interval_turns: default_interval_turns(),
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
        assert_eq!(empty.reflection.interval_turns, 10);
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
                "interval_turns": 5,
                "auto_apply_suggestions": true,
                "model": "test/model"
            }
        }"#;
        let config: MemoryConfig = serde_json::from_str(json).unwrap();

        assert!(!config.enabled);
        assert_eq!(config.max_entries, 20);
        assert_eq!(config.similarity_threshold, 0.6);
        assert!(!config.reflection.enabled);
        assert_eq!(config.reflection.interval_turns, 5);
        assert!(config.reflection.auto_apply_suggestions);
        assert_eq!(config.reflection.model.as_deref(), Some("test/model"));
    }

    #[test]
    fn test_reflection_config_default() {
        let config = ReflectionConfig::default();

        assert!(config.enabled);
        assert_eq!(config.interval_turns, 10);
        assert!(!config.auto_apply_suggestions);
        assert!(config.model.is_none());
    }
}
