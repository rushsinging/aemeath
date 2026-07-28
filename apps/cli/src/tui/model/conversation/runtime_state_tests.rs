use super::*;
use crate::tui::model::conversation::status_notice::StatusNoticeKind;

#[test]
fn graph_phase_idle_is_ready_success() {
    let notice = RuntimeState::notice_from_phase(Some("idle"));

    assert_eq!(notice.text, "Ready");
    assert_eq!(notice.kind, StatusNoticeKind::Success);
}

#[test]
fn graph_phase_non_idle_is_running_without_text_guessing() {
    for phase in ["thinking", "tool_call", "custom-phase"] {
        let notice = RuntimeState::notice_from_phase(Some(phase));

        assert_eq!(notice.text, phase);
        assert_eq!(notice.kind, StatusNoticeKind::Running);
    }
}
