use super::constants::MAX_CHARS;

pub(super) fn diagnostic_plain(value: &str) -> String {
    let mut out = value
        .chars()
        .take(MAX_CHARS)
        .collect::<String>()
        .replace('\n', "\\n");
    if value.chars().count() > MAX_CHARS {
        out.push('…');
    }
    out
}
