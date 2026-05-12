pub(super) fn truncate_for_spinner(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

pub(super) fn short_hook_command(command: &str) -> String {
    let trimmed = command.trim().trim_matches('"');
    let tail = trimmed.rsplit('/').next().unwrap_or(trimmed);
    truncate_for_spinner(tail, 48)
}
