//! ReasoningGraph 配置反序列化与覆盖逻辑。
//!
//! 配置结构对应 `aemeath.json` 中的 `reasoning_graph` 段：
//!
//! ```json
//! {
//!   "reasoning_graph": {
//!     "enabled": true,
//!     "max_reasoning": "high",
//!     "nodes": {
//!       "explore": { "effort": "medium" },
//!       "plan":    { "effort": "high" },
//!       "execute": { "effort": "low" },
//!       "verify":  { "effort": "medium" }
//!     }
//!   }
//! }
//! ```

use provider::api::ReasoningLevel;
use serde::{Deserialize, Serialize};

/// 单个节点的 effort 覆盖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEffortConfig {
    pub effort: ReasoningLevel,
}

/// 全部节点的 effort 覆盖映射。任一字段缺失则使用默认值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodesConfig {
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
    /// 总开关。`false` 时 graph 不干预 effort（回退到 max_reasoning 固定值）。
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 用户允许的最大 reasoning 深度。未指定时默认 `Max`（不限制）。
    #[serde(default = "default_max_reasoning")]
    pub max_reasoning: ReasoningLevel,

    /// 各节点的 effort 覆盖。
    #[serde(default)]
    pub nodes: NodesConfig,
}

fn default_enabled() -> bool {
    false
}

fn default_max_reasoning() -> ReasoningLevel {
    ReasoningLevel::Max
}

impl Default for ReasoningGraphConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_reasoning: default_max_reasoning(),
            nodes: NodesConfig::default(),
        }
    }
}

impl ReasoningGraphConfig {
    /// 返回指定节点的 effort（优先用配置覆盖，否则返回节点默认值）。
    pub fn effort_for(&self, node: super::ReasoningNode) -> ReasoningLevel {
        use super::ReasoningNode::*;
        let override_val = match node {
            Explore => self.nodes.explore.as_ref().map(|c| c.effort),
            Plan => self.nodes.plan.as_ref().map(|c| c.effort),
            Execute => self.nodes.execute.as_ref().map(|c| c.effort),
            Verify => self.nodes.verify.as_ref().map(|c| c.effort),
            Idle => return ReasoningLevel::Off,
        };
        override_val.unwrap_or_else(|| node.default_effort())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_disabled() {
        let config = ReasoningGraphConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_reasoning, ReasoningLevel::Max);
    }

    #[test]
    fn test_effort_for_uses_defaults_when_no_override() {
        let config = ReasoningGraphConfig::default();
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Explore),
            ReasoningLevel::Medium
        );
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Plan),
            ReasoningLevel::High
        );
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Execute),
            ReasoningLevel::Low
        );
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Verify),
            ReasoningLevel::Medium
        );
    }

    #[test]
    fn test_effort_for_uses_override_when_present() {
        let config = ReasoningGraphConfig {
            enabled: true,
            max_reasoning: ReasoningLevel::Max,
            nodes: NodesConfig {
                explore: Some(NodeEffortConfig {
                    effort: ReasoningLevel::High,
                }),
                plan: None,
                execute: None,
                verify: None,
            },
        };
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Explore),
            ReasoningLevel::High
        );
    }

    #[test]
    fn test_idle_always_off() {
        let config = ReasoningGraphConfig::default();
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Idle),
            ReasoningLevel::Off
        );
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
        assert_eq!(config.max_reasoning, ReasoningLevel::High);
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Explore),
            ReasoningLevel::Low
        );
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Execute),
            ReasoningLevel::Off
        );
        // plan/verify 未覆盖 → 用默认值
        assert_eq!(
            config.effort_for(super::super::ReasoningNode::Plan),
            ReasoningLevel::High
        );
    }
}
