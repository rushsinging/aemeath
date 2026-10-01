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
