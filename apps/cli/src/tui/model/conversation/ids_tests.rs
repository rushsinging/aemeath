use super::{ChatId, ChatTurnId, ToolCallId};

#[test]
fn tui_id_preserves_the_acl_string_identity() {
    let chat = ChatId::new("chat-from-runtime");
    let turn = ChatTurnId::new("turn-from-runtime");
    let tool = ToolCallId::new("tool-from-runtime");

    assert_eq!(chat.as_str(), "chat-from-runtime");
    assert_eq!(turn.as_str(), "turn-from-runtime");
    assert_eq!(tool.as_str(), "tool-from-runtime");
}
