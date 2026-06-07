use std::borrow::Cow;

const OPEN_TAG: &str = "<system-reminder>";
const CLOSE_TAG: &str = "</system-reminder>";

pub fn strip_system_reminder_envelope(text: &str) -> Cow<'_, str> {
    let trimmed = text.trim();
    if !trimmed.starts_with(OPEN_TAG) || !trimmed.ends_with(CLOSE_TAG) {
        return Cow::Borrowed(text);
    }

    let inner = &trimmed[OPEN_TAG.len()..trimmed.len() - CLOSE_TAG.len()];
    Cow::Owned(inner.trim().to_string())
}

pub fn strip_system_reminder_envelope_owned(text: String) -> String {
    match strip_system_reminder_envelope(&text) {
        Cow::Borrowed(_) => text,
        Cow::Owned(stripped) => stripped,
    }
}

#[cfg(test)]
mod tests {
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
}
