use super::*;

#[test]
fn test_payload_message_str() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
    assert_eq!(payload_message(payload.as_ref()), "boom");
}

#[test]
fn test_payload_message_string() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("kaboom"));
    assert_eq!(payload_message(payload.as_ref()), "kaboom");
}

#[test]
fn test_payload_message_unknown() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
    assert_eq!(payload_message(payload.as_ref()), "unknown panic");
}

#[test]
fn test_terminal_restore_seq_contains_leave_altscreen_and_show_cursor() {
    // \x1b[?1049l = LeaveAlternateScreen, \x1b[?25h = show cursor
    assert!(TERMINAL_RESTORE_SEQ.windows(8).any(|w| w == b"\x1b[?1049l"));
    assert!(TERMINAL_RESTORE_SEQ.windows(6).any(|w| w == b"\x1b[?25h"));
}
