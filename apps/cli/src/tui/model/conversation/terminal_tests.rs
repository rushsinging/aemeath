use super::{terminal_notice, TerminalCause, DONE_VERBS};
use std::time::Duration;

#[test]
fn completed_notice_uses_canonical_done_verb_and_duration() {
    let notice = terminal_notice(TerminalCause::Completed, Some(Duration::from_secs(125)))
        .expect("completed terminal notice");

    assert!(notice.starts_with("✻ "));
    assert!(notice.ends_with(" for 2m 5s"));
    assert!(DONE_VERBS
        .iter()
        .any(|verb| notice == format!("✻ {verb} for 2m 5s")));
    assert!(!notice.contains("Completed"));
}

#[test]
fn cancelled_notice_preserves_canonical_text_with_and_without_duration() {
    assert_eq!(
        terminal_notice(TerminalCause::UserCancelled, Some(Duration::from_secs(125))),
        Some("✻ Cancelled, ran 2m 5s".to_string())
    );
    assert_eq!(
        terminal_notice(TerminalCause::UserCancelled, None),
        Some("✻ Cancelled".to_string())
    );
}

#[test]
fn terminated_notice_preserves_canonical_text() {
    assert_eq!(
        terminal_notice(TerminalCause::RunTerminated, Some(Duration::from_secs(5))),
        Some("此 Run 已终止".to_string())
    );
}
