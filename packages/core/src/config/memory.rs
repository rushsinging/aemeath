//! Memory 系统配置

use serde::{Deserialize, Serialize};

pub(crate) fn default_max_entries() -> usize {
    100
}

pub(crate) fn default_max_inject_count() -> usize {
    10
}

pub(crate) fn default_similarity_threshold() -> f64 {
    0.8
}

pub(crate) fn default_interval_turns() -> usize {
    10
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

    /// Maximum memory entries injected into system prompt.
    #[serde(default = "default_max_inject_count")]
    pub max_inject_count: usize,

    /// Enable automatic summary on session end.
    #[serde(default = "super::ui::default_true")]
    pub auto_summary_on_session_end: bool,

    /// Similarity threshold for deduplication.
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,

    /// Reflection configuration.
    #[serde(default)]
    pub reflection: ReflectionConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_max_entries(),
            max_inject_count: default_max_inject_count(),
            auto_summary_on_session_end: true,
            similarity_threshold: default_similarity_threshold(),
            reflection: ReflectionConfig::default(),
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
        assert_eq!(config.max_inject_count, 10);
        assert!(config.auto_summary_on_session_end);
        assert_eq!(config.similarity_threshold, 0.8);
        assert!(config.reflection.enabled);
    }

    #[test]
    fn test_memory_config_deserialize_empty() {
        let config: MemoryConfig = serde_json::from_str("{}").unwrap();

        assert!(config.enabled);
        assert_eq!(config.max_entries, 100);
        assert_eq!(config.reflection.interval_turns, 10);
        assert!(!config.reflection.auto_apply_suggestions);
    }

    #[test]
    fn test_memory_config_deserialize_custom() {
        let json = r#"{
            "enabled": false,
            "max_entries": 20,
            "max_inject_count": 3,
            "auto_summary_on_session_end": false,
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
        assert_eq!(config.max_inject_count, 3);
        assert!(!config.auto_summary_on_session_end);
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
