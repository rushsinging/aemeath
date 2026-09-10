use super::*;

#[test]
fn status_notice_default_is_ready_normal() {
    let notice = StatusNotice::default();

    assert_eq!(notice.text, "Ready");
    assert_eq!(notice.kind, StatusNoticeKind::Normal);
}

#[test]
fn status_notice_success_sets_kind_and_text() {
    let notice = StatusNotice::success("Copied");

    assert_eq!(notice.text, "Copied");
    assert_eq!(notice.kind, StatusNoticeKind::Success);
}

#[test]
fn status_notice_warning_sets_kind_and_text() {
    let notice = StatusNotice::warning("Interrupted");

    assert_eq!(notice.text, "Interrupted");
    assert_eq!(notice.kind, StatusNoticeKind::Warning);
}
