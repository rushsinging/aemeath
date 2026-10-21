use super::SessionReasoningControl;
use sdk::ThinkingRequest;
use share::reasoning::ReasoningLevel;

#[test]
fn model_default_control_reads_requested_and_is_not_user_selected() {
    let control = SessionReasoningControl::from_model_default(ReasoningLevel::High);

    assert_eq!(control.requested(), ReasoningLevel::High);
    assert!(!control.user_selected());
}

#[test]
fn user_selection_marks_control_user_selected() {
    let control = SessionReasoningControl::from_user_selection(ReasoningLevel::Max);

    assert_eq!(control.requested(), ReasoningLevel::Max);
    assert!(control.user_selected());
}

#[test]
fn apply_level_request_overrides_requested_and_marks_user_selected() {
    let mut control = SessionReasoningControl::from_model_default(ReasoningLevel::Medium);

    let applied = control.apply_thinking_request(ThinkingRequest::Level(ReasoningLevel::Xhigh));

    assert_eq!(applied, ReasoningLevel::Xhigh);
    assert_eq!(control.requested(), ReasoningLevel::Xhigh);
    assert!(control.user_selected());
}

#[test]
fn apply_on_request_maps_to_medium() {
    let mut control = SessionReasoningControl::from_model_default(ReasoningLevel::Off);

    let applied = control.apply_thinking_request(ThinkingRequest::On);

    assert_eq!(applied, ReasoningLevel::Medium);
    assert!(control.user_selected());
}

#[test]
fn apply_off_request_maps_to_off() {
    let mut control = SessionReasoningControl::from_user_selection(ReasoningLevel::High);

    let applied = control.apply_thinking_request(ThinkingRequest::Off);

    assert_eq!(applied, ReasoningLevel::Off);
    assert!(control.user_selected());
}

#[test]
fn apply_toggle_flips_between_off_and_last_enabled_level() {
    let mut control = SessionReasoningControl::from_user_selection(ReasoningLevel::Xhigh);

    // toggle 从显式高档位 → 关闭。
    assert_eq!(
        control.apply_thinking_request(ThinkingRequest::Toggle),
        ReasoningLevel::Off
    );
    // 再 toggle → 回到 Medium（无历史记忆的开档默认，与旧语义一致）。
    assert_eq!(
        control.apply_thinking_request(ThinkingRequest::Toggle),
        ReasoningLevel::Medium
    );
}

#[test]
fn model_switch_keeps_user_selection_and_resets_default_when_not_selected() {
    let mut user_selected = SessionReasoningControl::from_user_selection(ReasoningLevel::Max);
    user_selected.on_model_switched(ReasoningLevel::Medium);
    assert_eq!(user_selected.requested(), ReasoningLevel::Max);
    assert!(user_selected.user_selected());

    let mut model_default = SessionReasoningControl::from_model_default(ReasoningLevel::High);
    model_default.on_model_switched(ReasoningLevel::Low);
    assert_eq!(model_default.requested(), ReasoningLevel::Low);
    assert!(!model_default.user_selected());
}
