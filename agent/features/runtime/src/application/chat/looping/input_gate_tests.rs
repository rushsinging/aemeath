//! `input_gate` 的测试模块，从 `input_gate.rs` 外提以降低文件体量。

use super::input_gate::*;
use crate::application::chat::looping::events::{ChatEventSink, EventFuture, RuntimeStreamEvent};
use crate::application::chat::looping::queue::{QueueDrainPort, QueueFuture};
use context::session::ChatChain;
use sdk::ChatInputEvent;
use share::message::Message;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// #567 S5：gate 测试用 task_store（run_loop_gate 新增参数）
pub(super) fn test_task_store() -> storage::TaskStore {
    storage::TaskStore::new()
}

/// Mock port backed by tokio mpsc; supports both drain and blocking recv.
#[derive(Clone)]
pub(super) struct MockInputPort {
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ChatInputEvent>>>,
}

impl MockInputPort {
    pub(super) fn new() -> (mpsc::UnboundedSender<ChatInputEvent>, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            tx,
            Self {
                rx: Arc::new(tokio::sync::Mutex::new(rx)),
            },
        )
    }
}

impl InputEventDrainPort for MockInputPort {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            rx.recv().await
        })
    }
}

#[tokio::test]
async fn test_recv_next_input_returns_event_then_none_on_close() {
    // MockInputPort: 用 tokio::sync::mpsc 支持 recv_next
    let (tx, port) = MockInputPort::new();
    tx.send(ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "hi".into(),
        images: vec![],
    })
    .unwrap();
    let first = port.recv_next_input().await;
    assert!(matches!(first, Some(ChatInputEvent::UserMessage { .. })));
    drop(tx); // 关闭通道
    let after_close = port.recv_next_input().await;
    assert!(after_close.is_none(), "通道关闭后返回 None=shutdown");
}

#[derive(Clone)]
pub(super) struct TestInputEventPort {
    events: Arc<Mutex<Vec<ChatInputEvent>>>,
}

impl TestInputEventPort {
    pub(super) fn new(events: Vec<ChatInputEvent>) -> Self {
        Self {
            events: Arc::new(Mutex::new(events)),
        }
    }
}

impl InputEventDrainPort for TestInputEventPort {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move { self.events.lock().unwrap().drain(..).collect() })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move {
            let mut events = self.events.lock().unwrap();
            if events.is_empty() {
                None
            } else {
                Some(events.remove(0))
            }
        })
    }
}

#[derive(Clone)]
pub(super) struct TestQueuePort {
    queued: Arc<Mutex<Option<Vec<String>>>>,
}

impl TestQueuePort {
    pub(super) fn new(queued: Option<Vec<String>>) -> Self {
        Self {
            queued: Arc::new(Mutex::new(queued)),
        }
    }
}

impl QueueDrainPort for TestQueuePort {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
        Box::pin(async move { self.queued.lock().unwrap().take() })
    }
}

#[derive(Clone, Default)]
pub(super) struct TestSink {
    pub(super) events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl ChatEventSink for TestSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn test_run_loop_gate_before_finish_continues_on_user_message() {
    let mut buffer = PendingInputBuffer::default();
    let queue = EmptyQueueDrainPort;
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message("继续", Vec::new())]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(vec![Message::user("first")]);

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::BeforeFinish,
        &mut buffer,
        &queue,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(chain.messages_flat().last().unwrap().text_content(), "继续");
    // 现在 append 时发 MessagesSync + UserMessagesAdopted 两个事件
    assert_eq!(sink.events.lock().unwrap().len(), 2);
}

/// #402 回归 + #fix-tui-image-input-output 拆块回归：
/// 带图 UserMessage 事件必须按 text 中 `[Image #N]` 占位符穿插组装，
/// 而非把所有 image 堆到 content 头部、text 堆到末尾。
#[tokio::test]
async fn test_user_message_with_images_assembles_image_block() {
    use share::message::{ContentBlock, ImageSource};
    let img = sdk::ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "Zm9vYmFy".to_string(),
        media_type: "image/png".to_string(),
    };
    // text 含 `[Image #1]` 占位符，期望 image 穿插到 text 中占位位置
    let text_with_marker = "看[Image #1]这张图".to_string();
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
        text_with_marker.clone(),
        vec![img],
    )]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.appended_user_messages, 1);
    let msgs = chain.messages_flat();
    let last = msgs.last().expect("应追加一条消息");
    // text_content 拼回完整文本（拆块后还原）
    assert_eq!(last.text_content(), text_with_marker);
    // 期望 content 是 [Text("看"), Image, Text("这张图")] 三块
    assert_eq!(
        last.content.len(),
        3,
        "期望拆成 3 块，实际={:?}",
        last.content
    );
    assert!(matches!(&last.content[0], ContentBlock::Text { text } if text == "看"));
    let has_image = last.content.iter().any(|block| {
        matches!(
            block,
            ContentBlock::Image {
                source: ImageSource::Base64 { data, media_type },
                placeholder: Some(ph),
            } if data == "Zm9vYmFy" && media_type == "image/png" && ph == "[Image #1]"
        )
    });
    assert!(
        has_image,
        "带图 UserMessage 应组装出 base64 image block（带 placeholder），实际 content={:?}",
        last.content
    );
    assert!(matches!(&last.content[2], ContentBlock::Text { text } if text == "这张图"));
}

/// #fix-tui-image-input-output：多图按 text 中 `[Image #N]` 出现顺序穿插。
#[tokio::test]
async fn test_user_message_with_multiple_images_interleaves_by_placeholder() {
    use share::message::ContentBlock;
    let imgs = vec![
        sdk::ChatInputImage {
            id: "[Image #1]".to_string(),
            base64: "a".to_string(),
            media_type: "image/png".to_string(),
        },
        sdk::ChatInputImage {
            id: "[Image #2]".to_string(),
            base64: "b".to_string(),
            media_type: "image/jpeg".to_string(),
        },
    ];
    // text 中 [Image #2] 在 [Image #1] 前面，期望穿插顺序: [Image #2], [Image #1]
    let text = "B: [Image #2], A: [Image #1]".to_string();
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(text.clone(), imgs)]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let _ = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    let msgs = chain.messages_flat();
    let last = msgs.last().expect("应追加一条消息");
    assert_eq!(last.text_content(), text);
    let placeholders: Vec<String> = last
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Image {
                placeholder: Some(p),
                ..
            } => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        placeholders,
        vec!["[Image #2]".to_string(), "[Image #1]".to_string()],
        "image 应按 text 中 `[Image #N]` 出现顺序穿插，实际 blocks={:?}",
        last.content
    );
}

#[tokio::test]
async fn test_run_loop_gate_after_blocking_appends_without_continue_decision() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
        "tool 后输入",
        Vec::new(),
    )]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::AfterBlockingBoundary,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::Proceed);
    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(chain.messages_flat()[0].text_content(), "tool 后输入");
}

#[tokio::test]
async fn test_run_loop_gate_preserves_side_effect_command_order() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("text1", Vec::new()),
        ChatInputEvent::ControlCommand {
            raw: "/save".to_string(),
        },
        ChatInputEvent::user_message("text2", Vec::new()),
    ]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
    assert_eq!(outcome.commands.len(), 1);
    assert_eq!(outcome.commands[0].raw, "/save");
    assert_eq!(chain.messages_flat()[0].text_content(), "text1");
    assert_eq!(chain.messages_flat()[1].text_content(), "text2");
}

#[tokio::test]
async fn test_run_loop_gate_clear_drops_following_events_and_prior_appends() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("text1", Vec::new()),
        ChatInputEvent::ControlCommand {
            raw: "/clear".to_string(),
        },
        ChatInputEvent::user_message("text2", Vec::new()),
    ]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::BeforeFinish,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::AbortCurrentLoop);
    assert_eq!(outcome.dropped_events, 1);
    assert_eq!(outcome.commands[0].kind, ControlCommandKind::Abort);
    assert!(chain.is_empty());
}

#[tokio::test]
async fn test_apply_gate_emits_user_messages_added_batch_no_dedup() {
    let mut buffer = PendingInputBuffer::default();
    // 含重复文本：验证不去重
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("same", Vec::new()),
        ChatInputEvent::user_message("same", Vec::new()),
    ]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.appended_user_messages, 2, "不去重：两条都 append");
    assert_eq!(chain.messages_flat().len(), 2);
    let added = sink.events.lock().unwrap().iter().find_map(|e| match e {
        RuntimeStreamEvent::UserMessagesAdopted { items, .. } => Some(items.clone()),
        _ => None,
    });
    let items = added.expect("应发出一个 UserMessagesAdopted 批事件");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].1.text_content(), "same");
    assert_eq!(items[1].1.text_content(), "same");
    assert_ne!(items[0].0, items[1].0, "每条提交一个独立 id");
}

#[tokio::test]
async fn test_run_loop_gate_no_dedup_push_and_pull_same_text() {
    // 设计 §8：不去重。queue 和 input 各来一条相同文本，两条都 append。
    let mut buffer = PendingInputBuffer::default();
    let queue = TestQueuePort::new(Some(vec!["same".to_string()]));
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message("same", Vec::new())]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(Vec::new());

    let task_store = test_task_store();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &queue,
        &input,
        &sink,
        &mut chain,
        "seg",
        &task_store,
        false,
    )
    .await;

    assert_eq!(outcome.appended_user_messages, 2, "不去重：两条都 append");
    assert_eq!(chain.messages_flat().len(), 2);
}

/// #391 S3-1：drain_all 非空 → 返回全部事件 + buffer 清空。
#[test]
fn test_drain_all_returns_all_events_and_clears() {
    let mut buffer = PendingInputBuffer::default();
    let a = ChatInputEvent::user_message("aaa", Vec::new());
    let b = ChatInputEvent::user_message("bbb", Vec::new());
    buffer.push(a.clone());
    buffer.push(b.clone());

    let drained = buffer.drain_all();

    assert_eq!(drained.len(), 2);
    assert!(matches!(&drained[0], ChatInputEvent::UserMessage { text, .. } if text == "aaa"));
    assert!(matches!(&drained[1], ChatInputEvent::UserMessage { text, .. } if text == "bbb"));
    assert!(buffer.is_empty(), "drain_all 后 buffer 应为空");
}

/// #391 S3-1：drain_all 空 → 返回空 Vec，buffer 仍空。
#[test]
fn test_drain_all_empty_returns_empty_vec() {
    let mut buffer = PendingInputBuffer::default();
    let drained = buffer.drain_all();
    assert!(drained.is_empty());
    assert!(buffer.is_empty());
}
