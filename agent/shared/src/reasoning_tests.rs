use super::ReasoningLevel;

#[test]
fn reasoning_level_parses_and_displays_all_levels() {
    for level in [
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
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
fn none_is_input_alias_for_off_with_canonical_off_output() {
    assert_eq!(ReasoningLevel::parse("none"), Some(ReasoningLevel::Off));
    assert_eq!(ReasoningLevel::parse("None"), Some(ReasoningLevel::Off));
    // canonical 输出仍是 "off"，round-trip 单拼写稳定。
    assert_eq!(ReasoningLevel::Off.as_str(), "off");
    assert_eq!(ReasoningLevel::Off.to_string(), "off");
}

#[test]
fn minimal_is_ordered_between_off_and_low() {
    assert!(ReasoningLevel::Off < ReasoningLevel::Minimal);
    assert!(ReasoningLevel::Minimal < ReasoningLevel::Low);
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
