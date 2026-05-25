use kernel::message::Message;

/// Build the user context message from CLAUDE.md content, wrapped in <system-reminder> tags.
pub(crate) fn build_user_context_message(claude_md: &str) -> Option<Message> {
    if claude_md.is_empty() {
        return None;
    }
    Some(Message::user(format!(
        "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{claude_md}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
    )))
}
