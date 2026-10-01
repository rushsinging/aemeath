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

pub(super) fn normalize_rendered_table_plain(plain: &str) -> String {
    let Some((left, right)) = plain.split_once('│') else {
        return plain.to_string();
    };
    format!("{}  │{}", left.trim_end(), right.trim_end())
}
