//! Chat 完成结果 / 事件流 / 工具结果图片。

use crate::chat_event::ChatEvent;

/// 工具结果中的图片载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultImage {
    pub base64: String,
    pub media_type: String,
}

/// Chat 完成结果。
#[derive(Debug, Clone)]
pub struct ChatResult {
    /// 最终响应文本。
    pub text: String,
    /// 本次 Chat 消耗的 token 数（如果可用）。
    pub tokens_used: Option<u64>,
}

/// Chat 事件流。
///
/// TUI 使用 `recv().await` 阻塞等待——终端事件循环是轮询模型。
pub struct ChatStream {
    rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>,
}

impl ChatStream {
    pub fn new(rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>) -> Self {
        Self { rx }
    }

    /// 接收下一个事件，流结束时返回 None。
    pub async fn recv(&mut self) -> Option<ChatEvent> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_event::{ChatEvent, ChatEventContext};

    #[tokio::test]
    async fn test_chat_stream_recv_returns_sent_event() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let chat_id = crate::ids::ChatId::new_v7();
        let turn_id = crate::ids::ChatTurnId::new_v7();
        tx.send(ChatEvent::Token {
            context: ChatEventContext::new(chat_id.clone(), turn_id.clone()),
            text: "hello".to_string(),
        })
        .unwrap();
        drop(tx);
        let mut stream = ChatStream::new(rx);

        let event = stream.recv().await;

        match event {
            Some(ChatEvent::Token { context, text }) => {
                assert_eq!(context.chat_id, chat_id);
                assert_eq!(context.turn_id, turn_id);
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
}
