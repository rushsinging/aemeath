use super::*;
use share::message::Message;

fn user_text(text: &str) -> ContextMessage {
    Message::user(text)
}

#[test]
fn map_messages_reuses_unchanged_committed_step_payload() {
    let original_step: Arc<[ContextMessage]> = vec![user_text("history")].into();
    let reusable_step = Arc::clone(&original_step);
    let messages =
        ContextMessages::from_committed_steps(vec![reusable_step], vec![user_text("pending")]);

    let mapped = messages.map_messages(|_| None);

    assert_eq!(mapped.len(), 2, "map 不得增删消息");
    assert!(
        Arc::ptr_eq(&mapped.committed_steps[0], &original_step),
        "无修改的 committed step 必须复用原 Arc payload"
    );
    assert_eq!(mapped[0].text_content(), "history");
}

#[test]
fn map_messages_replaces_modified_messages_and_preserves_order() {
    let messages = ContextMessages::from_committed_steps(
        vec![vec![user_text("history")].into()],
        vec![user_text("pending")],
    );

    let mapped = messages.map_messages(|message| {
        if message.role == share::message::Role::User {
            Some(Message::user(format!("[T] {}", message.text_content())))
        } else {
            None
        }
    });

    assert_eq!(mapped.len(), 2, "map 不得增删消息");
    assert_eq!(mapped[0].text_content(), "[T] history");
    assert_eq!(mapped[1].text_content(), "[T] pending");
}

#[test]
fn map_messages_modifies_committed_and_keeps_pending_untouched() {
    let messages = ContextMessages::from_committed_steps(
        vec![vec![user_text("history")].into()],
        vec![user_text("pending")],
    );

    let mapped = messages.map_messages(|message| {
        (message.text_content() == "history").then(|| Message::user("kept"))
    });

    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped[0].text_content(), "kept");
    assert_eq!(
        mapped[1].text_content(),
        "pending",
        "未命中的 pending 必须原样保留"
    );
}
