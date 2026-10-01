use super::constants::{CLOSE_TAG, OPEN_TAG};
use std::borrow::Cow;

pub fn strip_system_reminder_envelope(text: &str) -> Cow<'_, str> {
    let trimmed = text.trim();
    let Some(after_open) = trimmed.strip_prefix(OPEN_TAG) else {
        return Cow::Borrowed(text);
    };
    let Some(inner) = after_open.strip_suffix(CLOSE_TAG) else {
        return Cow::Borrowed(text);
    };
    Cow::Owned(inner.trim().to_string())
}

pub fn strip_system_reminder_envelope_owned(text: String) -> String {
    match strip_system_reminder_envelope(&text) {
        Cow::Borrowed(_) => text,
        Cow::Owned(stripped) => stripped,
    }
}

#[cfg(test)]
#[path = "system_reminder_tests.rs"]
mod tests;
