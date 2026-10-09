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
