//! Reasoning Graph 配置段。
//!
//! 对应 `aemeath.json` 中的 `reasoning_graph` 段：
//!
//! ```json
//! {
//!   "reasoning_graph": {
//!     "enabled": true,
//!     "max_reasoning": "high",
//!     "nodes": {
//!       "explore": { "effort": "low" },
//!       "plan":    { "effort": "high" },
//!       "execute": { "effort": "medium" },
//!       "verify":  { "effort": "high" }
//!     } // 显式 override 示例，不代表 Workflow 节点默认值
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};

/// 单个节点的 effort 覆盖。`effort` 为字符串（`"off"`/`"low"`/`"medium"`/`"high"`/`"xhigh"`/`"max"`），
/// 由 Workflow BC parse 为 `ReasoningLevel`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEffortConfig {
    pub effort: String,
}

/// 全部节点的 effort 覆盖映射。任一字段缺失则使用默认值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReasoningGraphNodesConfig {
    #[serde(default)]
    pub explore: Option<NodeEffortConfig>,
    #[serde(default)]
    pub plan: Option<NodeEffortConfig>,
    #[serde(default)]
    pub execute: Option<NodeEffortConfig>,
    #[serde(default)]
    pub verify: Option<NodeEffortConfig>,
}

/// Reasoning Graph 顶层配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningGraphConfig {
    /// 总开关。`false` 时 graph 不干预 effort。
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 用户允许的最大 reasoning 深度（`"off"`/`"low"`/…/`"max"`）。
    /// 未指定时默认 `"max"`（不限制）。
    #[serde(default = "default_max_reasoning")]
    pub max_reasoning: String,

    /// 各节点的 effort 覆盖。
    #[serde(default)]
    pub nodes: ReasoningGraphNodesConfig,
}

fn default_enabled() -> bool {
    false
}

fn default_max_reasoning() -> String {
    "max".to_string()
}

impl Default for ReasoningGraphConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_reasoning: default_max_reasoning(),
            nodes: ReasoningGraphNodesConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_disabled() {
        let config = ReasoningGraphConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_reasoning, "max");
    }

    #[test]
    fn test_deserialize_full_config() {
        let json = r#"{
            "enabled": true,
            "max_reasoning": "high",
            "nodes": {
                "explore": { "effort": "low" },
                "execute": { "effort": "off" }
            }
        }"#;
        let config: ReasoningGraphConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.max_reasoning, "high");
        assert_eq!(config.nodes.explore.as_ref().unwrap().effort, "low");
        assert_eq!(config.nodes.execute.as_ref().unwrap().effort, "off");
        assert!(config.nodes.plan.is_none());
        assert!(config.nodes.verify.is_none());
    }

    #[test]
    fn test_deserialize_empty_uses_defaults() {
        let json = r#"{}"#;
        let config: ReasoningGraphConfig = serde_json::from_str(json).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.max_reasoning, "max");
    }
}
