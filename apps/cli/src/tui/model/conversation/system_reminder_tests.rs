use super::strip_system_reminder_envelope;

#[test]
fn strips_complete_envelope() {
    assert_eq!(
        strip_system_reminder_envelope("<system-reminder>\nabc\n</system-reminder>"),
        "abc"
    );
}

#[test]
fn strips_outer_whitespace() {
    assert_eq!(
        strip_system_reminder_envelope("  <system-reminder>abc</system-reminder>  "),
        "abc"
    );
}

#[test]
fn leaves_plain_text_unchanged() {
    assert_eq!(strip_system_reminder_envelope("abc"), "abc");
}

#[test]
fn leaves_incomplete_open_tag_unchanged() {
    assert_eq!(
        strip_system_reminder_envelope("<system-reminder>abc"),
        "<system-reminder>abc"
    );
}

#[test]
fn leaves_incomplete_close_tag_unchanged() {
    assert_eq!(
        strip_system_reminder_envelope("abc</system-reminder>"),
        "abc</system-reminder>"
    );
}

#[test]
fn leaves_embedded_tag_unchanged() {
    assert_eq!(
        strip_system_reminder_envelope("prefix <system-reminder>abc</system-reminder>"),
        "prefix <system-reminder>abc</system-reminder>"
    );
}

#[test]
fn strip_envelope_with_attributes() {
    // #1695 envelope 化：开标签携带 kind/version/at/seq 属性。
    let text = "<system-reminder kind=\"background-process\" version=\"1\" at=\"2026-10-10T05:01:20Z\" seq=\"1\">\nbody line\n</system-reminder>";
    assert_eq!(strip_system_reminder_envelope(text), "body line");
}
