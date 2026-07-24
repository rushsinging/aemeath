use super::{reasoning_level_from_options, ReasoningConfig};
use crate::ReasoningLevel;

#[test]
fn thinking_budget_only_controls_disabled_or_enabled_fallback_level() {
    assert_eq!(
        reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(0))),
        ReasoningLevel::Off
    );
    assert_eq!(
        reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(1))),
        ReasoningLevel::High
    );
    assert_eq!(
        reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(40_000))),
        ReasoningLevel::High
    );
}
