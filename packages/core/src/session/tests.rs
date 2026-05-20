use super::*;
use crate::message::{ContentBlock, Message, Role};

fn make_session(title: Option<&str>, project: Option<&str>, messages: Vec<Message>) -> Session {
    let mut sess = Session::new("test-id".into(), "/tmp".into());
    if let Some(t) = title {
        sess.set_title(t.to_string());
    }
    if let Some(p) = project {
        sess.metadata.project = Some(p.to_string());
    }
    sess.messages = messages;
    sess
}

#[test]
fn summary_with_title() {
    let sess = make_session(Some("My Session Title"), None, vec![]);
    assert_eq!(sess.summary(), "My Session Title");
}

#[test]
fn summary_with_long_title_truncated() {
    let long = "a".repeat(50);
    let sess = make_session(Some(&long), None, vec![]);
    assert_eq!(sess.summary().len(), 40);
    assert_eq!(sess.summary(), "a".repeat(40));
}

#[test]
fn summary_with_first_user_message() {
    let msgs = vec![Message::user("Hello, this is my first message")];
    let sess = make_session(None, None, msgs);
    assert_eq!(sess.summary(), "Hello, this is my first message");
}

#[test]
fn summary_with_long_user_message_truncated() {
    let long_msg = "This is a very long user message that should be truncated at fifty characters for display purposes";
    let msgs = vec![Message::user(long_msg)];
    let sess = make_session(None, None, msgs);
    assert_eq!(sess.summary().len(), 50);
}

#[test]
fn summary_with_multiline_user_message() {
    let msgs = vec![Message::user("First line of message\nSecond line")];
    let sess = make_session(None, None, msgs);
    assert_eq!(sess.summary(), "First line of message");
}

#[test]
fn summary_with_only_assistant_messages() {
    let msgs = vec![Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "assistant says".into(),
        }],
    }];
    let sess = make_session(None, None, msgs);
    // no user message → falls back to project name (extracted from "/tmp")
    assert_eq!(sess.summary(), "tmp");
}

#[test]
fn summary_with_empty_user_message() {
    let msgs = vec![Message::user("")];
    let sess = make_session(None, None, msgs);
    // empty user message → falls back to project name
    assert_eq!(sess.summary(), "tmp");
}

#[test]
fn summary_no_title_no_messages_no_project() {
    let mut sess = Session::new("test-id".into(), "/tmp".into());
    sess.metadata.project = None; // clear project set by Session::new
    sess.summary(); // should not panic
    assert_eq!(sess.summary(), "unknown");
}

#[test]
fn summary_title_overrides_user_message() {
    let msgs = vec![Message::user("user message content")];
    let sess = make_session(Some("Custom Title"), None, msgs);
    // title should take priority over user message
    assert_eq!(sess.summary(), "Custom Title");
}

#[test]
fn summary_project_fallback() {
    let sess = make_session(None, Some("my-project"), vec![]);
    assert_eq!(sess.summary(), "my-project");
}

#[test]
fn summary_no_messages() {
    let sess = make_session(None, None, vec![]);
    // no messages → project "tmp" from Session::new("/tmp")
    assert_eq!(sess.summary(), "tmp");
}
