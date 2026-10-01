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
