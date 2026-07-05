//! Chat 完成结果 / 事件流 / 工具结果图片 / 用户输入图片。

use crate::chat_event::ChatEvent;

/// 工具结果中的图片载荷。
///
/// 命名语义：**工具执行结果返回**的图片（runtime → TUI），承载 base64 + media_type。
/// 与 `ChatInputImage`（用户输入图片，TUI → Runtime）严格区分。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolResultImage {
    pub base64: String,
    pub media_type: String,
}

/// TUI → Runtime 通道的用户输入图片（#fix-tui-image-input-output）。
///
/// `id` 为占位符字符串（形如 `"[Image #1]"`），由 TUI 端 `ImageSpan::placeholder()`
/// 生成，用于 runtime 在拆分 text 与 image 时一一配对：
/// `text` 中出现的 `[Image #N]` ↔ `images` 中 `id == "[Image #N]"`。
///
/// **不发给 LLM**——provider adapter 拿 runtime 拆好的 `Vec<ContentBlock>` 后丢弃 `id`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatInputImage {
    pub id: String,
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

/// 取消句柄：TUI 持有，触发即向 runtime 的 CancellationToken 发**即时**取消信号
/// （进程内 out-of-band，NEVER 走事件流——避免工具/hook/compact 期间的排队延迟）。
///
/// 用闭包封装，让 SDK 契约层不依赖 `tokio_util::CancellationToken`：runtime 侧
/// （`trait_chat::chat_impl`）构造时注入「锁共享 cancel 槽 + cancel 当前 token」的
/// 闭包，TUI 侧只认这个句柄。#639。
#[derive(Clone)]
pub struct CancelHandle {
    trigger: std::sync::Arc<dyn Fn() + Send + Sync>,
}

impl CancelHandle {
    /// 用触发闭包构造（runtime 侧调用）。
    pub fn new(trigger: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            trigger: std::sync::Arc::new(trigger),
        }
    }

    /// 无操作句柄——测试 / 无 runtime 的 mock ChatStream 用。
    pub fn noop() -> Self {
        Self {
            trigger: std::sync::Arc::new(|| {}),
        }
    }

    /// 触发取消（即时）。幂等：重复调用无害（CancellationToken 已取消再 cancel 无副作用）。
    pub fn cancel(&self) {
        (self.trigger)();
    }
}

impl std::fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CancelHandle")
    }
}

/// Chat 事件流。
///
/// TUI 使用 `recv().await` 阻塞等待——终端事件循环是轮询模型。
/// 附带 [`CancelHandle`]：TUI 在 Ctrl+C/Esc 时调 `cancel_handle().cancel()` 即时中断本次 chat。
pub struct ChatStream {
    rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>,
    cancel: CancelHandle,
}

impl ChatStream {
    /// 无 cancel 能力的流（测试 / mock 用）。
    pub fn new(rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>) -> Self {
        Self {
            rx,
            cancel: CancelHandle::noop(),
        }
    }

    /// 带 cancel 句柄的流（runtime 生产路径用）。
    pub fn with_cancel(
        rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>,
        cancel: CancelHandle,
    ) -> Self {
        Self { rx, cancel }
    }

    /// 取出可 clone 的取消句柄（TUI 存入 ProcessingHandle，Ctrl+C 时触发）。
    pub fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
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

    #[test]
    fn test_cancel_handle_invokes_trigger_closure() {
        // #639：cancel() 必须调用注入的闭包（runtime 侧靠它触发 CancellationToken）。
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handle = CancelHandle::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(count.load(Ordering::SeqCst), 0);
        handle.cancel();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // 幂等：重复 cancel 不 panic（token 已取消再取消无副作用）。
        handle.cancel();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_cancel_handle_clone_shares_trigger() {
        // cancel_handle() 返回 clone，两个 clone 触发同一闭包。
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let handle = CancelHandle::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        let cloned = handle.clone();
        handle.cancel();
        cloned.cancel();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_chat_stream_with_cancel_exposes_handle() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let fired = Arc::new(AtomicBool::new(false));
        let f = fired.clone();
        let stream = ChatStream::with_cancel(
            rx,
            CancelHandle::new(move || f.store(true, Ordering::SeqCst)),
        );
        stream.cancel_handle().cancel();
        assert!(fired.load(Ordering::SeqCst));
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
}
