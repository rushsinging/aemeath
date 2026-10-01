//! `input_gate` 的测试模块，从 `input_gate.rs` 外提以降低文件体量。

use super::input_gate::*;
use crate::application::loop_engine::chat::events::{
    ChatEventSink, EventFuture, RuntimeStreamEvent,
};
use sdk::ChatInputEvent;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

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
async fn skill_request_adoption_preserves_typed_display_payload() {
    let buffer = PendingInputBuffer::default();
    let input_id = sdk::InputId::new_v7();
    let input = TestInputEventPort::new(vec![ChatInputEvent::SkillRequest(sdk::SkillRequest {
        input_id: input_id.clone(),
        skill: "superpowers:brainstorming".to_string(),
        arguments: "feature scope".to_string(),
        raw_input: "/superpowers:brainstorming feature scope".to_string(),
    })]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.accepted_inputs.len(), 1);
    assert_eq!(outcome.accepted_inputs[0].input_id(), &input_id);
    let accepted_message = outcome.accepted_inputs[0].model_message();
    assert_eq!(
        accepted_message.source(),
        share::message::MessageSource::SkillRequest
    );
    assert_eq!(
        accepted_message
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.skill_request.as_ref()),
        Some(&share::message::SkillRequestMetadata {
            skill: "superpowers:brainstorming".to_string(),
            arguments: "feature scope".to_string(),
            raw_input: "/superpowers:brainstorming feature scope".to_string(),
        })
    );
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

/// 除排队快照外发出的语义事件数量（#1816）。
///
/// `ControlCommandsQueued` 是队列状态同步，不改变 gate 的业务判定；
/// 断言「gate 不发 Adopted / SessionMessageStateChanged」时必须排除它，
/// 否则会把新增的状态同步误判成语义回归。
fn semantic_event_count(sink: &TestSink) -> usize {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| !matches!(event, RuntimeStreamEvent::ControlCommandsQueued { .. }))
        .count()
}

#[tokio::test]
async fn compact_input_becomes_idle_command_and_is_buffered_while_busy() {
    let idle_buffer = PendingInputBuffer::default();
    idle_buffer.push(ChatInputEvent::Compact);
    let idle_outcome = apply_gate(
        GateKind::BeforeLlm,
        &idle_buffer,
        &TestSink::default(),
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert!(matches!(
        idle_outcome.pending_command,
        Some(PendingCommand::Compact)
    ));
    assert!(idle_buffer.is_empty());

    let busy_buffer = PendingInputBuffer::default();
    busy_buffer.push(ChatInputEvent::Compact);
    let busy_outcome = apply_gate(
        GateKind::BeforeLlm,
        &busy_buffer,
        &TestSink::default(),
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert!(busy_outcome.pending_command.is_none());
    assert!(!busy_buffer.is_empty());
}

/// `/reflect-now` idle 受理为 PendingCommand；busy 直接提示丢弃，NEVER 排队。
#[tokio::test]
async fn reflect_now_idle_becomes_pending_command_and_busy_drops_with_notice() {
    let idle_buffer = PendingInputBuffer::default();
    idle_buffer.push(ChatInputEvent::ReflectNow);
    let idle_sink = TestSink::default();
    let idle_outcome = apply_gate(
        GateKind::BeforeLlm,
        &idle_buffer,
        &idle_sink,
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert!(matches!(
        idle_outcome.pending_command,
        Some(PendingCommand::ReflectNow)
    ));
    assert!(idle_buffer.is_empty(), "idle 受理后事件消费完毕");
    assert_eq!(
        semantic_event_count(&idle_sink),
        0,
        "受理提示由 run_launch handler 发出，gate 不重复提示"
    );

    let busy_buffer = PendingInputBuffer::default();
    busy_buffer.push(ChatInputEvent::ReflectNow);
    let busy_sink = TestSink::default();
    let busy_outcome = apply_gate(
        GateKind::BeforeLlm,
        &busy_buffer,
        &busy_sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert!(busy_outcome.pending_command.is_none());
    assert!(busy_buffer.is_empty(), "busy 不排队：事件被丢弃");
    let busy_events = busy_sink.events.lock().unwrap();
    // 排队快照是状态同步（#1816），busy 丢弃路径另发一条空快照。
    let semantic: Vec<_> = busy_events
        .iter()
        .filter(|event| !matches!(event, RuntimeStreamEvent::ControlCommandsQueued { .. }))
        .collect();
    match semantic.as_slice() {
        [RuntimeStreamEvent::CommandResultText { text, is_error }] => {
            assert!(!is_error, "busy 跳过是提示而非错误");
            assert!(text.contains("Reflection"), "提示应说明跳过原因：{text}");
        }
        other => panic!("busy 应只发一条 CommandResultText，实际 {other:?}"),
    }
}

#[tokio::test]
async fn test_run_loop_gate_before_finish_continues_on_user_message() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message("继续", Vec::new())]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeFinish,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(outcome.accepted_inputs.len(), 1);
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "继续"
    );
    // #1272: apply_gate no longer emits Adopted or SessionMessageStateChanged.
    // Adopted is deferred to accept_step_input after durable Context accept.
    // #1816: 排队快照是状态同步，不计入语义事件。
    assert_eq!(semantic_event_count(&sink), 0);
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
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
        text_with_marker.clone(),
        vec![img],
    )]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(outcome.accepted_inputs.len(), 1);
    let last = outcome.accepted_inputs[0].model_message();
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
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(text.clone(), imgs)]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.accepted_inputs.len(), 1);
    let last = outcome.accepted_inputs[0].model_message();
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
async fn query_reflection_history_is_buffered_while_gate_is_busy() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::QueryReflectionHistory { limit: 7 }]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert!(outcome.pending_command.is_none());
    assert!(outcome.accepted_inputs.is_empty());
    assert!(matches!(
        buffer.drain_all().as_slice(),
        [ChatInputEvent::QueryReflectionHistory { limit: 7 }]
    ));
}

#[tokio::test]
async fn test_run_loop_gate_after_blocking_appends_without_continue_decision() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
        "tool 后输入",
        Vec::new(),
    )]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::AfterBlockingBoundary,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::Proceed);
    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(outcome.accepted_inputs.len(), 1);
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "tool 后输入"
    );
}

#[tokio::test]
async fn test_run_loop_gate_preserves_side_effect_command_order() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("text1", Vec::new()),
        ChatInputEvent::ControlCommand {
            raw: "/save".to_string(),
        },
        ChatInputEvent::user_message("text2", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
    assert_eq!(outcome.commands.len(), 1);
    assert_eq!(outcome.commands[0].raw, "/save");
    assert_eq!(outcome.accepted_inputs.len(), 2);
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "text1"
    );
    assert_eq!(
        outcome.accepted_inputs[1].model_message().text_content(),
        "text2"
    );
}

#[tokio::test]
async fn test_run_loop_gate_clear_drops_following_events_and_prior_appends() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("text1", Vec::new()),
        ChatInputEvent::ControlCommand {
            raw: "/clear".to_string(),
        },
        ChatInputEvent::user_message("text2", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeFinish,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::AbortCurrentLoop);
    assert_eq!(outcome.dropped_events, 1);
    assert_eq!(outcome.commands[0].kind, ControlCommandKind::Abort);
    assert!(outcome.accepted_inputs.is_empty());
}

#[tokio::test]
async fn test_apply_gate_emits_user_messages_added_batch_no_dedup() {
    let buffer = PendingInputBuffer::default();
    // 含重复文本：验证不去重
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("same", Vec::new()),
        ChatInputEvent::user_message("same", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    // #1818 改写：原断言逐条比对两条 accepted message，固化「同批逐条接纳」。
    // 「不去重」的保护目标改由「合并后的文本仍是两份 same」覆盖——重复内容
    // 不会被丢弃，只是不再拆成两条结构。
    assert_eq!(outcome.appended_user_messages, 1, "同批折叠为一条");
    assert_eq!(outcome.accepted_inputs.len(), 1);
    // #1272: apply_gate no longer emits UserMessagesAdopted.
    // Adopted data is carried in outcome.accepted_inputs for RunPort.
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "same\n\nsame",
        "重复文本的两条消息都要保留，不能因内容相同被丢弃"
    );
    // #1818 改写：原断言「每条提交一个独立 id」在同批折叠后不再成立——合并消息
    // 沿用批次首条 InputId（其余 id 随合并失去独立意义，TUI 消费走全量替换，
    // 不依赖 id 匹配）。跨批次的 id 独立性由 accepted_message 与 input_id
    // 保留测试覆盖。
    // Verify no semantic events were emitted through the sink（快照除外，#1816）。
    assert_eq!(semantic_event_count(&sink), 0);
}

#[tokio::test]
async fn test_run_loop_gate_preserves_duplicate_typed_events() {
    // 同一 typed ingress 中的两条相同文本具有不同 InputId，均必须被采用。
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("same", Vec::new()),
        ChatInputEvent::user_message("same", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    // #1818 改写：同批两条相同文本的消息折叠为一条，但两份内容都保留在
    // 合并文本里——「不去重」的保护目标不变。
    assert_eq!(outcome.appended_user_messages, 1, "同批折叠为一条");
    assert_eq!(outcome.accepted_inputs.len(), 1);
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "same\n\nsame"
    );
}

/// #391 S3-1：drain_all 非空 → 返回全部事件 + buffer 清空。
#[test]
fn test_drain_all_returns_all_events_and_clears() {
    let buffer = PendingInputBuffer::default();
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

/// #1272 回归：apply_gate accepted_inputs 携带原始 ChatInputEvent（含 InputId + images）。
/// Gate 不再 emit UserMessagesAdopted，数据由 accepted_inputs 传入 Run 经 accept_step_input 后 emit。
#[tokio::test]
async fn test_apply_gate_accepted_inputs_preserve_input_id_and_images() {
    let img = sdk::ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "Zm9vYmFy".to_string(),
        media_type: "image/png".to_string(),
    };
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
        "看图[Image #1]".to_string(),
        vec![img.clone()],
    )]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert_eq!(outcome.appended_user_messages, 1);
    // Gate 不再 emit 语义事件（Adopted deferred to accept_step_input；快照除外，#1816）
    assert_eq!(semantic_event_count(&sink), 0);
    // accepted_inputs 唯一保留 InputId、images 与模型消息。
    assert_eq!(outcome.accepted_inputs.len(), 1);
    match &outcome.accepted_inputs[0] {
        crate::application::loop_engine::AcceptedUserInput::UserMessage {
            input_id,
            text,
            images,
        } => {
            assert_eq!(text, "看图[Image #1]");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].id, "[Image #1]");
            assert_eq!(images[0].base64, "Zm9vYmFy");
            assert_eq!(outcome.accepted_inputs[0].input_id(), input_id);
        }
        other => panic!("expected typed UserMessage input, got {other:?}"),
    }
    // typed input 生成的模型消息包含 image content block。
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "看图[Image #1]"
    );
}

/// #1272：apply_gate 不再 emit UserMessagesAdopted / SessionMessageStateChanged。
/// Gate 的 adopted data 仅供 RunPort 携带，UI 投影由 accept_step_input 后发出。
#[tokio::test]
async fn test_apply_gate_no_premature_adopted_emission() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("hello", Vec::new()),
        ChatInputEvent::user_message("world", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    // #1818 改写：原断言 `appended_user_messages == 2` / `accepted_inputs.len() == 2`
    // 固化了「同批逐条接纳」的旧结构。替代证据是本测试断言两条消息折叠为一条
    // 且文本按空行拼接——两条内容都没有丢，只是结构上成为一条连续输入。
    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(outcome.accepted_inputs.len(), 1);
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "hello\n\nworld"
    );
    // 确认没有语义事件通过 sink 发出（排队快照是状态同步，#1816）
    assert_eq!(
        semantic_event_count(&sink),
        0,
        "apply_gate must not emit semantic events; Adopted is deferred to accept_step_input"
    );
}

// ── 用户输入时刻盖章（metadata.created_at）──────────────────────

#[tokio::test]
async fn model_message_stamps_user_input_timestamp_for_plain_input() {
    let buffer = PendingInputBuffer::default();
    let input_id = sdk::InputId::new_v7();
    let input = TestInputEventPort::new(vec![ChatInputEvent::UserMessage {
        id: input_id.clone(),
        text: "hello".to_string(),
        images: vec![],
    }]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;
    assert_eq!(outcome.accepted_inputs.len(), 1);

    let before = chrono::Local::now().fixed_offset();
    let message = outcome.accepted_inputs[0].model_message();
    let after = chrono::Local::now().fixed_offset();

    let created_at = message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.created_at)
        .expect("用户输入消息必须盖输入时刻");
    assert!(
        created_at >= before && created_at <= after,
        "created_at {created_at:?} 应落在构造调用时间区间 [{before:?}, {after:?}] 内"
    );
    assert_eq!(message.text_content(), "hello");
}

#[test]
fn loop_input_message_without_accepted_stamps_user_input_timestamp() {
    let loop_input = crate::application::loop_engine::engine::LoopInput {
        text: "hi".to_string(),
        input_id: None,
        images: vec![],
        accepted: None,
    };

    let before = chrono::Local::now().fixed_offset();
    let message = loop_input.message();
    let after = chrono::Local::now().fixed_offset();

    let created_at = message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.created_at)
        .expect("无 accepted 的 LoopInput 消息必须盖输入时刻");
    assert!(
        created_at >= before && created_at <= after,
        "created_at {created_at:?} 应落在构造调用时间区间 [{before:?}, {after:?}] 内"
    );
    assert_eq!(message.text_content(), "hi");
}

/// #1816：idle 受理控制命令后，剩余事件必须原序重新排队，NEVER 静默丢弃。
#[tokio::test]
async fn control_command_requeues_following_commands_instead_of_dropping_them() {
    let buffer = PendingInputBuffer::default();
    buffer.push(ChatInputEvent::Compact);
    buffer.push(ChatInputEvent::SwitchModel {
        selection: "anthropic/claude".to_string(),
    });

    let outcome = apply_gate(
        GateKind::BeforeLlm,
        &buffer,
        &TestSink::default(),
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert!(matches!(
        outcome.pending_command,
        Some(PendingCommand::Compact)
    ));
    let remaining = buffer.drain_all();
    assert_eq!(
        remaining,
        vec![ChatInputEvent::SwitchModel {
            selection: "anthropic/claude".to_string(),
        }],
        "首个命令之后的命令必须留待下一轮 idle 执行"
    );
}

/// #1816：命令之前的用户消息被接纳，命令之后的消息留待下一轮，NEVER 丢弃。
#[tokio::test]
async fn user_messages_around_control_command_are_split_not_dropped() {
    let buffer = PendingInputBuffer::default();
    buffer.push(ChatInputEvent::user_message("before", Vec::new()));
    buffer.push(ChatInputEvent::Compact);
    buffer.push(ChatInputEvent::user_message("after", Vec::new()));

    let outcome = apply_gate(
        GateKind::BeforeLlm,
        &buffer,
        &TestSink::default(),
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert!(matches!(
        outcome.pending_command,
        Some(PendingCommand::Compact)
    ));
    assert_eq!(outcome.appended_user_messages, 1);
    assert_eq!(outcome.accepted_inputs.len(), 1);
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "before"
    );
    let remaining = buffer.drain_all();
    assert_eq!(
        remaining,
        vec![ChatInputEvent::user_message("after", Vec::new())],
        "命令之后的消息必须留待下一轮，而不是被丢弃"
    );
}

/// #1816：idle Reset 之前已接纳、之后未消费的用户消息都必须回到缓冲区，
/// `reset_requested` 优先级高于 `Resumed`，否则它们会被静默丢弃。
#[tokio::test]
async fn user_messages_around_reset_are_requeued_instead_of_silently_dropped() {
    let buffer = PendingInputBuffer::default();
    buffer.push(ChatInputEvent::user_message("before", Vec::new()));
    buffer.push(ChatInputEvent::Reset);
    buffer.push(ChatInputEvent::user_message("after", Vec::new()));

    let outcome = apply_gate(
        GateKind::BeforeLlm,
        &buffer,
        &TestSink::default(),
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert!(outcome.reset_requested, "idle Reset 应请求清空会话");
    assert!(
        outcome.accepted_inputs.is_empty(),
        "Reset 优先于 Resumed，已接纳输入必须回到缓冲区"
    );
    let remaining = buffer.drain_all();
    assert_eq!(
        remaining,
        vec![
            ChatInputEvent::user_message("before", Vec::new()),
            ChatInputEvent::user_message("after", Vec::new()),
        ],
        "Reset 前后的消息都必须留待下一轮"
    );
}

/// #1816：busy gate 把命令放回缓冲区后必须发布全量快照，UI 才知道命令已排队。
#[tokio::test]
async fn busy_gate_publishes_command_queue_snapshot() {
    let buffer = PendingInputBuffer::default();
    let sink = TestSink::default();

    buffer.push(ChatInputEvent::Compact);
    buffer.push(ChatInputEvent::SwitchModel {
        selection: "anthropic/claude".to_string(),
    });
    let outcome = apply_gate(
        GateKind::BeforeLlm,
        &buffer,
        &sink,
        &task::TaskStore::new(),
        false,
    )
    .await;

    assert!(outcome.pending_command.is_none(), "busy 不执行命令");
    let events = sink.events.lock().unwrap();
    let snapshot = events
        .iter()
        .find_map(|event| match event {
            RuntimeStreamEvent::ControlCommandsQueued { queued } => Some(queued.clone()),
            _ => None,
        })
        .expect("gate 必须发布命令队列快照");
    assert_eq!(
        snapshot
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>(),
        vec!["/compact", "/model anthropic/claude"],
        "快照按入队顺序给出全部待执行命令"
    );
    assert!(
        snapshot[0].0 < snapshot[1].0,
        "入队序号必须单调，UI 据此跨队列排序"
    );
}

/// #1816：idle 消费掉命令后快照变空，UI 据此清空命令行。
#[tokio::test]
async fn idle_gate_publishes_empty_snapshot_after_consuming_command() {
    let buffer = PendingInputBuffer::default();
    let sink = TestSink::default();
    buffer.push(ChatInputEvent::Compact);

    apply_gate(
        GateKind::BeforeLlm,
        &buffer,
        &sink,
        &task::TaskStore::new(),
        true,
    )
    .await;

    let events = sink.events.lock().unwrap();
    let snapshot = events
        .iter()
        .find_map(|event| match event {
            RuntimeStreamEvent::ControlCommandsQueued { queued } => Some(queued.clone()),
            _ => None,
        })
        .expect("gate 必须发布命令队列快照");
    assert!(snapshot.is_empty(), "命令被消费后快照必须为空");
}

/// #1816：WithdrawAll 必须一并撤回排队的控制命令，Up 键才等于「全部撤回」。
#[tokio::test]
async fn withdraw_all_retracts_queued_control_commands_too() {
    let buffer = PendingInputBuffer::default();
    buffer.push(ChatInputEvent::user_message("queued", Vec::new()));
    buffer.push(ChatInputEvent::WithdrawAll);
    buffer.push(ChatInputEvent::Compact);
    buffer.push(ChatInputEvent::SwitchModel {
        selection: "anthropic/claude".to_string(),
    });
    let sink = TestSink::default();

    let outcome = apply_gate(
        GateKind::BeforeLlm,
        &buffer,
        &sink,
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert!(outcome.pending_command.is_none(), "撤回后不得执行命令");
    assert!(
        buffer.is_empty(),
        "控制命令必须随撤回离开队列，NEVER 留在队列里执行"
    );
    let snapshot = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            RuntimeStreamEvent::ControlCommandsQueued { queued } => Some(queued.clone()),
            _ => None,
        })
        .expect("gate 必须发布命令队列快照");
    assert!(snapshot.is_empty(), "撤回后命令队列快照必须为空");
}

/// #1816：撤回排队控制命令的 buffer 级原语——取出展示文本并清空队列。
#[test]
fn pending_buffer_drain_for_withdraw_returns_command_texts_and_clears_queue() {
    let buffer = PendingInputBuffer::default();
    buffer.push(ChatInputEvent::Compact);
    buffer.push(ChatInputEvent::user_message("queued", Vec::new()));
    buffer.push(ChatInputEvent::SwitchModel {
        selection: "anthropic/claude".to_string(),
    });

    let withdrawn = buffer.drain_for_withdraw();

    assert_eq!(
        withdrawn,
        vec![
            "/compact".to_string(),
            "/model anthropic/claude".to_string()
        ],
        "撤回必须给出命令展示文本，TUI 才能还原输入框"
    );
    assert!(buffer.is_empty(), "撤回后队列必须清空");
    assert!(buffer.command_snapshot().is_empty());
}

/// #1818：同一批 gate 里的连续用户消息合并为一条，文本空行分隔。
#[tokio::test]
async fn apply_gate_merges_consecutive_user_messages_in_one_batch() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("第一段", Vec::new()),
        ChatInputEvent::user_message("第二段", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert_eq!(outcome.accepted_inputs.len(), 1, "同批连续消息折叠为一条");
    assert_eq!(
        outcome.accepted_inputs[0].model_message().text_content(),
        "第一段\n\n第二段"
    );
    assert_eq!(outcome.appended_user_messages, 1);
}

/// #1818：SkillRequest 是合并边界，其两侧消息各自折叠。
#[tokio::test]
async fn apply_gate_keeps_skill_request_as_merge_boundary() {
    let buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("技能之前", Vec::new()),
        ChatInputEvent::SkillRequest(sdk::SkillRequest {
            input_id: sdk::InputId::new_v7(),
            skill: "superpowers:brainstorming".to_string(),
            arguments: "scope".to_string(),
            raw_input: "/superpowers:brainstorming scope".to_string(),
        }),
        ChatInputEvent::user_message("技能之后", Vec::new()),
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &buffer,
        &input,
        &sink,
        &task::TaskStore::new(),
        true,
    )
    .await;

    assert_eq!(
        outcome.accepted_inputs.len(),
        3,
        "技能两侧的消息各自只有一条，折叠后仍是三条，技能不被吸收进消息"
    );
    assert!(matches!(
        &outcome.accepted_inputs[1],
        crate::application::loop_engine::AcceptedUserInput::SkillRequest(_)
    ));
}
