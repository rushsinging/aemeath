//! Memory 系统配置

use serde::{Deserialize, Serialize};

pub(crate) fn default_max_entries() -> usize {
    100
}

pub(crate) fn default_similarity_threshold() -> f64 {
    0.8
}

pub(crate) fn default_interval_runs() -> usize {
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

    /// Similarity threshold for deduplication.
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,

    /// Reflection configuration.
    #[serde(default)]
    pub reflection: ReflectionConfig,

    /// 自动 Memory 注入的 token 预算覆盖（#1777）。
    ///
    /// `None`（默认）表示按窗口比例计算（`context_size / 50`，即 2%）；
    /// `Some(0)` 显式禁用自动注入。部分数值则直接作为固定预算。
    ///
    /// 条数上限 `inject_count` 已随比例化移除：预算本身就是上限，再叠一个
    /// 条数约束只会让「长条目被条数截断、短条目被预算截断」两种语义互相
    /// 掩盖。旧配置中的 `inject_count` 残留被忽略（serde 不拒绝未知字段）。
    #[serde(default)]
    pub inject_token_budget: Option<usize>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_max_entries(),
            similarity_threshold: default_similarity_threshold(),
            reflection: ReflectionConfig::default(),
            inject_token_budget: None,
        }
    }
}

/// Reflection system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionConfig {
    /// Enable reflection system.
    #[serde(default = "super::ui::default_true")]
    pub enabled: bool,

    /// Trigger reflection every N runs（每个 session 内每 N 个 Run 触发一次；
    /// 计数不跨 session/clear 延续）。
    #[serde(default = "default_interval_runs", alias = "interval_run_steps")]
    pub interval_runs: usize,

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
            interval_runs: default_interval_runs(),
            auto_apply_suggestions: false,
            model: None,
        }
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;

#[cfg(test)]
mod interval_runs_compat_tests {
    use super::*;

    /// 旧键 `interval_run_steps` 仍是合法读取别名：存量配置文件零迁移。
    #[test]
    fn legacy_interval_run_steps_key_still_deserializes() {
        let json = r#"{
            "reflection": { "interval_run_steps": 7 }
        }"#;
        let config: MemoryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.reflection.interval_runs, 7);
    }

    /// 新键 `interval_runs` 正常读取；缺省回落默认 10。
    #[test]
    fn interval_runs_key_deserializes_and_defaults() {
        let json = r#"{ "reflection": { "interval_runs": 3 } }"#;
        let config: MemoryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.reflection.interval_runs, 3);

        let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.reflection.interval_runs, 10);
    }
}
