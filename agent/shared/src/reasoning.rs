//! 跨 BC 共享的 reasoning 深度级别。

use serde::{Deserialize, Serialize};

/// Workflow、Provider、Runtime 等上下文交换的稳定 reasoning 级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    Off,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::Xhigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    pub fn clamped_to(self, max: Self) -> Self {
        self.min(max)
    }
}

impl std::fmt::Display for ReasoningLevel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::ReasoningLevel;

    #[test]
    fn reasoning_level_parses_and_displays_all_levels() {
        for level in [
            ReasoningLevel::Off,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
            ReasoningLevel::Xhigh,
            ReasoningLevel::Max,
        ] {
            assert_eq!(ReasoningLevel::parse(level.as_str()), Some(level));
            assert_eq!(level.to_string(), level.as_str());
        }
        assert_eq!(ReasoningLevel::parse("invalid"), None);
    }

    #[test]
    fn reasoning_level_clamps_to_maximum() {
        assert_eq!(
            ReasoningLevel::Xhigh.clamped_to(ReasoningLevel::Medium),
            ReasoningLevel::Medium
        );
        assert_eq!(
            ReasoningLevel::Low.clamped_to(ReasoningLevel::High),
            ReasoningLevel::Low
        );
    }
}
