use share::message::Message;

/// Extract the messages that were injected by the system (not from the session history)
/// plus the last persisted message for input logging purposes.
pub fn logged_input_messages(
    messages_for_api: &[Message],
    persisted_message_count: usize,
) -> Vec<serde_json::Value> {
    let injected_count = messages_for_api
        .len()
        .saturating_sub(persisted_message_count);
    let mut indices: Vec<usize> = (0..injected_count).collect();
    if persisted_message_count > 0 && !messages_for_api.is_empty() {
        indices.push(messages_for_api.len() - 1);
    }
    indices
        .into_iter()
        .filter_map(|index| messages_for_api.get(index))
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
                "len": m.content.len(),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "input_log_tests.rs"]
mod tests;
