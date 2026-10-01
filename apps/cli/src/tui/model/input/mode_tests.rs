use super::*;

#[test]
fn test_input_mode_default_is_normal() {
    assert_eq!(InputMode::default(), InputMode::Normal);
}

#[test]
fn test_input_mode_completion_is_distinct() {
    assert_ne!(InputMode::Completion, InputMode::Normal);
}
