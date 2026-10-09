//! idle 生命周期的 wakeup 接线测试（#252 PR2）。

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::idle_lifecycle::{idle_until_resume_or_shutdown, IdleResult};
use crate::application::loop_engine::chat::input_gate::{
    InputEventDrainPort, InputEventFuture, InputEventOptFuture, PendingInputBuffer,
};
use crate::application::loop_engine::chat::{ChatEventSink, RuntimeStreamEvent};
use sdk::ChatInputEvent;

/// 输入源替身：无事件，recv 永久 pending（不关闭通道、不产生输入）。
#[derive(Clone)]
struct NeverInputSource {
    _anchor: Arc<()>,
}

impl NeverInputSource {
    fn new() -> Self {
        Self {
            _anchor: Arc::new(()),
        }
    }
}

impl InputEventDrainPort for NeverInputSource {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move { Vec::new() })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        // 永久 pending：wake 后仍 Pending（无输入语义）。
        let pending: Pin<Box<dyn std::future::Future<Output = Option<ChatInputEvent>> + Send>> =
            Box::pin(std::future::pending());
        pending
    }
}

/// 脚本化输入源：预置事件按序返回，耗尽后永久 pending。
#[derive(Clone)]
struct ScriptedInputSource {
    received: Arc<Mutex<VecDeque<ChatInputEvent>>>,
}

impl InputEventDrainPort for ScriptedInputSource {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move {
            self.received
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .drain(..)
                .collect()
        })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        let next = self
            .received
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front();
        Box::pin(async move {
            match next {
                Some(event) => Some(event),
                None => std::future::pending().await,
            }
        })
    }
}

#[derive(Default, Clone)]
struct NullSink;

impl ChatEventSink for NullSink {
    fn send_event<'a>(
        &'a self,
        _event: RuntimeStreamEvent,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }

    fn try_send_event(&self, _event: RuntimeStreamEvent) {}
}

fn user_event(text: &str) -> ChatInputEvent {
    ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: text.to_string(),
        images: Vec::new(),
    }
}

#[tokio::test]
async fn idle_returns_background_process_wakeup_when_signal_arrives() {
    let (notifier, mut waiter) = crate::application::session::wakeup::wakeup_channel();
    let mut pending = PendingInputBuffer::default();
    let source = NeverInputSource::new();
    let sink = NullSink;
    let task_store = task::TaskStore::new();

    // 先投递信号（unbounded：recv 前已排队，select 立即命中唤醒分支）。
    notifier.wakeup().unwrap();
    let result =
        idle_until_resume_or_shutdown(&source, &sink, &mut pending, &task_store, Some(&mut waiter))
            .await;
    assert!(
        matches!(result, IdleResult::BackgroundProcessWakeup),
        "唤醒信号应驱动 Wakeup Run 结果，实际 {result:?}"
    );
}

#[tokio::test]
async fn idle_without_wakeup_waiter_keeps_existing_behavior() {
    // 不接入 wakeup（None）时：脚本化用户消息照常走 Resumed 路径。
    let source = ScriptedInputSource {
        received: Arc::new(Mutex::new(VecDeque::from(vec![user_event("hello")]))),
    };
    let mut pending = PendingInputBuffer::default();
    pending.push(ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "hello".to_string(),
        images: Vec::new(),
    });

    let result = idle_until_resume_or_shutdown(
        &source,
        &NullSink,
        &mut pending,
        &task::TaskStore::new(),
        None,
    )
    .await;
    match result {
        IdleResult::Resumed {
            accepted_inputs, ..
        } => {
            assert_eq!(accepted_inputs.len(), 1, "用户消息照常受理");
        }
        other => panic!("无 wakeup 时应走既有 Resumed 路径，实际 {other:?}"),
    }
}

#[allow(dead_code)]
fn assert_future_types_compatible(
    source: &NeverInputSource,
) -> (InputEventFuture<'_>, InputEventOptFuture<'_>) {
    (source.drain_input_events(), source.recv_next_input())
}

#[allow(dead_code)]
const fn _static_assert_sink_clone_send_sync()
where
    NullSink: Clone + Send + Sync + 'static,
{
}
