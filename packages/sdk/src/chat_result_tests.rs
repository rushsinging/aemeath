use super::*;
use crate::chat_event::{ChatEvent, ChatEventContext};

#[tokio::test]
async fn test_chat_stream_recv_returns_sent_event() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let chat_id = crate::ids::ChatId::new_v7();
    let run_id = crate::ids::ChatRunId::new_v7();
    tx.send(ChatEvent::Token {
        context: ChatEventContext::new(chat_id.clone(), run_id.clone()),
        text: "hello".to_string(),
    })
    .unwrap();
    drop(tx);
    let mut stream = ChatStream::new(rx);

    let event = stream.recv().await;

    match event {
        Some(ChatEvent::Token { context, text }) => {
            assert_eq!(context.chat_id, chat_id);
            assert_eq!(context.run_id, run_id);
            assert_eq!(text, "hello");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn test_chat_stream_recv_returns_none_after_sender_dropped() {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    drop(_tx);
    let mut stream = ChatStream::new(rx);

    assert!(stream.recv().await.is_none());
}

#[test]
fn test_tool_result_image_keeps_base64_and_media_type() {
    let image = ToolResultImage {
        base64: "abc".to_string(),
        media_type: "image/png".to_string(),
    };

    assert_eq!(image.base64, "abc");
    assert_eq!(image.media_type, "image/png");
}

#[test]
fn test_chat_input_image_keeps_id_and_payload() {
    let image = ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "abc".to_string(),
        media_type: "image/png".to_string(),
    };

    assert_eq!(image.id, "[Image #1]");
    assert_eq!(image.base64, "abc");
    assert_eq!(image.media_type, "image/png");
}
