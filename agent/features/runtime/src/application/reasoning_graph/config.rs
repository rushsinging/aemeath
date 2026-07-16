//! ReasoningGraph 运行时配置。
//!
//! 从 `share::config::ReasoningGraphConfig`（字符串 effort 值）转换为
//! 运行时使用的 `provider::ReasoningLevel` 枚举。

use provider::ReasoningLevel;
use share::config::ReasoningGraphConfig as SharedConfig;

use super::ReasoningNode;

/// 运行时 ReasoningGraph 配置（effort 已 parse 为 ReasoningLevel）。
#[derive(Debug, Clone)]
pub struct GraphRuntimeConfig {
    pub enabled: bool,
    pub max_reasoning: ReasoningLevel,
    pub explore_effort: Option<ReasoningLevel>,
    pub plan_effort: Option<ReasoningLevel>,
    pub execute_effort: Option<ReasoningLevel>,
    pub verify_effort: Option<ReasoningLevel>,
}

impl GraphRuntimeConfig {
    /// 从 shared 配置转换。无效 effort 字符串静默回退到默认值。
    pub fn from_shared(config: &SharedConfig) -> Self {
        Self {
            enabled: config.enabled,
            max_reasoning: parse_level(&config.max_reasoning).unwrap_or(ReasoningLevel::Max),
            explore_effort: config
                .nodes
                .explore
                .as_ref()
                .and_then(|c| parse_level(&c.effort)),
            plan_effort: config
                .nodes
                .plan
                .as_ref()
                .and_then(|c| parse_level(&c.effort)),
            execute_effort: config
                .nodes
                .execute
                .as_ref()
                .and_then(|c| parse_level(&c.effort)),
            verify_effort: config
                .nodes
                .verify
                .as_ref()
                .and_then(|c| parse_level(&c.effort)),
        }
    }

    /// 返回指定节点的 effort（优先用覆盖，否则用节点默认值）。
    pub fn effort_for(&self, node: ReasoningNode) -> ReasoningLevel {
        let override_val = match node {
            ReasoningNode::Explore => self.explore_effort,
            ReasoningNode::Plan => self.plan_effort,
            ReasoningNode::Execute => self.execute_effort,
            ReasoningNode::Verify => self.verify_effort,
            ReasoningNode::Idle => return ReasoningLevel::Off,
        };
        override_val.unwrap_or_else(|| node.default_effort())
    }
}

impl Default for GraphRuntimeConfig {
    fn default() -> Self {
        Self::from_shared(&SharedConfig::default())
    }
}

fn parse_level(s: &str) -> Option<ReasoningLevel> {
    match s.trim().to_lowercase().as_str() {
        "off" => Some(ReasoningLevel::Off),
        "low" => Some(ReasoningLevel::Low),
        "medium" => Some(ReasoningLevel::Medium),
        "high" => Some(ReasoningLevel::High),
        "xhigh" => Some(ReasoningLevel::Xhigh),
        "max" => Some(ReasoningLevel::Max),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::ReasoningGraphConfig as SharedConfig;

    #[test]
    fn test_default_disabled() {
        let config = GraphRuntimeConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_reasoning, ReasoningLevel::Max);
    }

    #[test]
    fn test_from_shared_parses_effort() {
        let shared = SharedConfig {
            enabled: true,
            max_reasoning: "high".to_string(),
            nodes: share::config::ReasoningGraphNodesConfig {
                explore: Some(share::config::NodeEffortConfig {
                    effort: "low".to_string(),
                }),
                ..Default::default()
            },
        };
        let config = GraphRuntimeConfig::from_shared(&shared);
        assert!(config.enabled);
        assert_eq!(config.max_reasoning, ReasoningLevel::High);
        assert_eq!(config.explore_effort, Some(ReasoningLevel::Low));
    }

    #[test]
    fn test_invalid_effort_string_silently_ignored() {
        let shared = SharedConfig {
            enabled: true,
            max_reasoning: "turbo".to_string(),
            nodes: share::config::ReasoningGraphNodesConfig {
                explore: Some(share::config::NodeEffortConfig {
                    effort: "invalid".to_string(),
                }),
                ..Default::default()
            },
        };
        let config = GraphRuntimeConfig::from_shared(&shared);
        // 无效 max_reasoning 回退到 Max
        assert_eq!(config.max_reasoning, ReasoningLevel::Max);
        // 无效 effort 覆盖 → None（使用默认值）
        assert_eq!(config.explore_effort, None);
    }

    #[test]
    fn test_effort_for_uses_override_when_present() {
        let config = GraphRuntimeConfig {
            enabled: true,
            max_reasoning: ReasoningLevel::Max,
            explore_effort: Some(ReasoningLevel::High),
            plan_effort: None,
            execute_effort: None,
            verify_effort: None,
        };
        assert_eq!(
            config.effort_for(ReasoningNode::Explore),
            ReasoningLevel::High
        );
        // plan 未覆盖 → 用默认值
        assert_eq!(config.effort_for(ReasoningNode::Plan), ReasoningLevel::Max);
    }

    #[test]
    fn test_idle_always_off() {
        let config = GraphRuntimeConfig::default();
        assert_eq!(config.effort_for(ReasoningNode::Idle), ReasoningLevel::Off);
    }
}
