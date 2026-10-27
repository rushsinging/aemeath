//! Wakeup Run 内部续延（#252）：空输入 Run 首次 drain 返回
//! `InternalContinuation(BackgroundProcessWakeup)` 使 engine 执行 step
//! （build_window 消费 reminder → LLM 调用），而非 `drain_or_seal`
//! 空批即 `EmptyAndSealed` 收口、完成事实随 Run 丢弃。

use super::*;
use crate::application::loop_engine::chat::{
    ChatEventSink, EventFuture, InputEventDrainPort, InputEventFuture, InputEventOptFuture,
    RuntimeStreamEvent,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct NoOpInput {
    deferred: Arc<Mutex<Vec<ChatInputEvent>>>,
}

impl InputEventDrainPort for NoOpInput {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move { Vec::new() })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move { None })
    }
}

impl SessionInputPort for NoOpInput {
    fn defer(&self, event: ChatInputEvent) {
        self.deferred.lock().unwrap().push(event);
    }
}

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn adapter_without_user_input() -> BufferedInputAdapter<NoOpInput> {
    BufferedInputAdapter {
        input_events: NoOpInput::default(),
        sink: ChatEventSinkHandle::new(RecordingSink::default()),
        pending_input: PendingInputBuffer::default(),
        run_input_buffer: crate::application::run::context::RunInputBufferHandle::new(),
        continuation: InputContinuationState::default(),
        run_id: share::ids::RunId::new_v7(),
        immediate_query:
            crate::application::loop_engine::chat::immediate_query::test_support::no_io_dispatcher(),
    }
}

#[tokio::test]
async fn wakeup_continuation_yields_internal_continuation_before_seal() {
    let mut adapter = adapter_without_user_input();
    adapter.continuation.install_background_process_wakeup();

    let outcome = adapter
        .drain_input(DrainEpoch(0))
        .await
        .expect("wakeup 续延 drain 不报错");
    match &outcome {
        DrainOutcome::InternalContinuation { kind, batch, epoch } => {
            assert_eq!(epoch, &DrainEpoch(0));
            assert!(
                batch.is_empty(),
                "wakeup Run 无用户输入，batch 应为空（step 由续延驱动）"
            );
            assert!(
                matches!(kind, InternalContinuationKind::BackgroundProcessWakeup),
                "kind 应为 BackgroundProcessWakeup，实际 {kind:?}"
            );
        }
        other => panic!("应返回 InternalContinuation，实际 {other:?}"),
    }

    // 续延一次性消费：后续 drain 回落 drain_or_seal 空批收口。
    let next = adapter
        .drain_input(DrainEpoch(1))
        .await
        .expect("续延消费后 drain 正常");
    assert!(
        matches!(next, DrainOutcome::EmptyAndSealed { epoch } if epoch == DrainEpoch(1)),
        "wakeup 续延只驱动一次 step，实际 {next:?}"
    );
}

#[tokio::test]
async fn wakeup_continuation_takes_priority_with_buffered_user_input() {
    let mut adapter = adapter_without_user_input();
    adapter.continuation.install_background_process_wakeup();
    let accepted_input = crate::application::loop_engine::AcceptedUserInput::from_event(
        ChatInputEvent::UserMessage {
            id: share::ids::InputId::new_v7(),
            text: "用户消息".to_string(),
            images: Vec::new(),
        },
    )
    .expect("用户消息可构造 accepted input");
    adapter
        .run_input_buffer
        .with_lock(|buffer| buffer.push_accepted(accepted_input));

    let outcome = adapter
        .drain_input(DrainEpoch(0))
        .await
        .expect("混合输入 drain 不报错");
    match outcome {
        DrainOutcome::InternalContinuation { kind, batch, .. } => {
            assert!(
                matches!(kind, InternalContinuationKind::BackgroundProcessWakeup),
                "kind 应为 BackgroundProcessWakeup"
            );
            assert_eq!(batch.len(), 1, "已缓冲用户消息应随续延批一起交付，不得丢弃");
        }
        other => panic!("应返回 InternalContinuation，实际 {other:?}"),
    }
}

#[tokio::test]
async fn without_wakeup_continuation_empty_input_seals_immediately() {
    let mut adapter = adapter_without_user_input();

    let outcome = adapter
        .drain_input(DrainEpoch(0))
        .await
        .expect("无续延 drain 正常");
    assert!(
        matches!(outcome, DrainOutcome::EmptyAndSealed { .. }),
        "无 wakeup 续延时空批照常收口（普通 Run 语义不变），实际 {outcome:?}"
    );
}

/// #2042 复现：turn 边界 drain 窗口收到的 `ListModels` 是 request/response
/// 查询事件，必须就地分派直接回 `ModelList`，NEVER 进入控制命令队列
/// （入队会被 TUI 回显为 `/model`，且 idle gate 兜底 drop 丢失刷新）。
#[tokio::test]
async fn drain_window_list_models_dispatches_immediately_without_queueing() {
    let scripted_input = ScriptedInput::new(vec![ChatInputEvent::ListModels]);
    let recording_sink = RecordingSink::default();
    let mut adapter = BufferedInputAdapter {
        input_events: scripted_input,
        sink: ChatEventSinkHandle::new(recording_sink.clone()),
        pending_input: PendingInputBuffer::default(),
        run_input_buffer: crate::application::run::context::RunInputBufferHandle::new(),
        continuation: InputContinuationState::default(),
        run_id: share::ids::RunId::new_v7(),
        immediate_query:
            crate::application::loop_engine::chat::immediate_query::test_support::no_io_dispatcher(),
    };

    let outcome = adapter
        .drain_input(DrainEpoch(0))
        .await
        .expect("drain 窗口含查询事件时 drain 不报错");

    // Run 语义本身不受查询事件影响：空批照常收口。
    assert!(
        matches!(outcome, DrainOutcome::EmptyAndSealed { .. }),
        "查询事件就地消费后空批照常收口，实际 {outcome:?}"
    );

    let events = recording_sink.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RuntimeStreamEvent::ModelList { .. })),
        "ListModels 应就地分派直接回 ModelList，实际事件: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, RuntimeStreamEvent::ControlCommandsQueued { .. })),
        "查询事件 NEVER 发布命令队列快照（否则 TUI 回显 /model），实际事件: {events:?}"
    );
    drop(events);
    assert!(
        adapter.pending_input.command_snapshot().is_empty(),
        "查询事件 NEVER 进入 pending_input，实际队列: {:?}",
        adapter.pending_input.command_snapshot()
    );
}

/// 按脚本吐出事件的输入源：drain 一次性返回脚本，recv 按序逐条返回。
#[derive(Clone, Default)]
struct ScriptedInput {
    drain_events: Arc<Mutex<Vec<ChatInputEvent>>>,
    recv_events: Arc<Mutex<std::collections::VecDeque<ChatInputEvent>>>,
}

impl ScriptedInput {
    fn new(events: Vec<ChatInputEvent>) -> Self {
        Self {
            drain_events: Arc::new(Mutex::new(events)),
            recv_events: Arc::new(Mutex::new(std::collections::VecDeque::new())),
        }
    }

    fn with_recv_events(events: Vec<ChatInputEvent>) -> Self {
        Self {
            drain_events: Arc::new(Mutex::new(Vec::new())),
            recv_events: Arc::new(Mutex::new(events.into())),
        }
    }
}

impl InputEventDrainPort for ScriptedInput {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move { self.drain_events.lock().unwrap().drain(..).collect() })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move { self.recv_events.lock().unwrap().pop_front() })
    }
}

impl SessionInputPort for ScriptedInput {
    fn defer(&self, event: ChatInputEvent) {
        // 查询事件消费路径 NEVER defer；记录以便断言未被误转移。
        self.drain_events.lock().unwrap().push(event);
    }
}

/// #2042 ingress 一致性契约：AwaitUser park 路径收到 `ListModels` 同样
/// 必须就地分派回 `ModelList` 并继续等待用户输入，NEVER 入队、不收口 Run。
#[tokio::test]
async fn await_user_input_list_models_dispatches_without_queueing_or_sealing() {
    let scripted_input = ScriptedInput::with_recv_events(vec![ChatInputEvent::ListModels]);
    let recording_sink = RecordingSink::default();
    let mut adapter = BufferedInputAdapter {
        input_events: scripted_input,
        sink: ChatEventSinkHandle::new(recording_sink.clone()),
        pending_input: PendingInputBuffer::default(),
        run_input_buffer: crate::application::run::context::RunInputBufferHandle::new(),
        continuation: InputContinuationState::default(),
        run_id: share::ids::RunId::new_v7(),
        immediate_query:
            crate::application::loop_engine::chat::immediate_query::test_support::no_io_dispatcher(),
    };

    let outcome = adapter
        .await_user_input(DrainEpoch(0))
        .await
        .expect("AwaitUser 查询事件路径不报错");

    // 查询事件消费后 recv 队列为空 → 源关闭收口（而非因查询事件本身收口）。
    assert!(
        matches!(outcome, DrainOutcome::EmptyAndSealed { .. }),
        "recv 源关闭才收口，实际 {outcome:?}"
    );

    let events = recording_sink.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RuntimeStreamEvent::ModelList { .. })),
        "AwaitUser 收到 ListModels 应就地分派回 ModelList，实际事件: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, RuntimeStreamEvent::ControlCommandsQueued { .. })),
        "查询事件 NEVER 发布命令队列快照，实际事件: {events:?}"
    );
    drop(events);
    assert!(
        adapter.pending_input.command_snapshot().is_empty(),
        "查询事件 NEVER 进入 pending_input，实际队列: {:?}",
        adapter.pending_input.command_snapshot()
    );
}
