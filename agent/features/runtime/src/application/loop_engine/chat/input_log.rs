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
mod tests {
    use super::*;
    use share::message::Message;

    #[test]
    fn test_logged_input_messages_happy_path_includes_latest_user_message() {
        let messages = vec![Message::user("context"), Message::user("hello")];

        let logged = logged_input_messages(&messages, 1);

        assert_eq!(logged.len(), 2);
        assert!(logged[0]["content"].to_string().contains("context"));
        assert!(logged[1]["content"].to_string().contains("hello"));
    }

    #[test]
    fn test_logged_input_messages_boundary_no_injected_message() {
        let messages = vec![Message::user("hello")];

        let logged = logged_input_messages(&messages, 1);

        assert_eq!(logged.len(), 1);
        assert!(logged[0]["content"].to_string().contains("hello"));
    }

    #[test]
    fn test_logged_input_messages_error_empty_input_is_empty() {
        let logged = logged_input_messages(&[], 0);

        assert!(logged.is_empty());
    }
}
