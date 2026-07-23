//! input_gate 的 Reset / WithdrawAll 语义测试（#391 系列）。
//!
//! 从 `input_gate_tests.rs` 拆出，复用其中的 harness（TestInputEventPort / TestSink 等）。

use super::input_gate_tests::{TestInputEventPort, TestSink};
use crate::application::main_loop::looping::events::RuntimeStreamEvent;
use crate::application::main_loop::looping::input_gate::{
    run_loop_gate, EmptyQueueDrainPort, GateDecision, GateKind, PendingInputBuffer,
};
use sdk::ChatInputEvent;
use task::TaskAccess;

/// #391 S1-3：idle gate 收到 Reset → 清空 messages + 发 SessionReset，保持 idle。
#[tokio::test]
async fn test_idle_gate_reset_clears_messages_and_emits_session_reset() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::Reset]);
    let sink = TestSink::default();

    let task_access = task::TaskStore::new();
    task_access
        .create_batch(task::BatchCreateSpec::try_new("request".into()).unwrap(), 1)
        .unwrap();
    task_access
        .create_task(
            task::TaskCreateSpec::try_new(
                "authoritative".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &task_access,
        true, // idle
    )
    .await;

    assert!(
        task_access.list().is_empty(),
        "Reset must clear authoritative TaskAccess"
    );
    assert!(task_access.list_batches().is_empty());
    assert_eq!(outcome.decision, GateDecision::Proceed);
    assert!(outcome.reset_requested, "idle Reset 应请求清空会话");
    assert!(
        outcome.adopted_messages.is_empty(),
        "idle Reset 清空后不应有 adopted 消息"
    );
    assert!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .all(|event| !matches!(event, RuntimeStreamEvent::SessionReset)),
        "durable Context clear 成功前 gate 不得提前发 SessionReset"
    );
}

/// #391 S1-3：busy gate 收到 Reset → 放回 buffer，messages 不变，等 idle 处理。
#[tokio::test]
async fn test_busy_gate_reset_defers_to_buffer() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::Reset]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &task::TaskStore::new(),
        false, // busy
    )
    .await;

    assert_eq!(buffer.len(), 1, "Reset 应留在 buffer 等待 idle");
    assert_eq!(outcome.decision, GateDecision::Proceed);
    assert!(!outcome.reset_requested, "busy gate 不应请求 reset");
    assert!(
        outcome.adopted_messages.is_empty(),
        "busy gate 不应 adopt 消息"
    );
    let has_reset = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, RuntimeStreamEvent::SessionReset));
    assert!(!has_reset, "busy gate 不应发 SessionReset");
}

/// #391 S1-3：idle Reset 后跟 UserMessage → 先清空再 append（Reset break，UserMessage 丢弃）。
#[tokio::test]
async fn test_idle_gate_reset_drops_following_events_in_same_batch() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::Reset,
        ChatInputEvent::user_message("after-reset", Vec::new()),
    ]);
    let sink = TestSink::default();

    let task_access = task::TaskStore::new();
    task_access
        .create_batch(task::BatchCreateSpec::try_new("request".into()).unwrap(), 1)
        .unwrap();
    task_access
        .create_task(
            task::TaskCreateSpec::try_new(
                "authoritative".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &task_access,
        true, // idle
    )
    .await;

    assert_eq!(outcome.dropped_events, 1, "Reset 后的 UserMessage 应被丢弃");
    assert!(outcome.reset_requested, "idle Reset 应请求清空会话");
    assert!(
        outcome.adopted_messages.is_empty(),
        "Reset 清空后不应 adopt 后续消息"
    );
}

/// #391 S3-3：WithdrawAll 非空 → 回滚已 append + 收集剩余 text + 发 Withdrawn。
#[tokio::test]
async fn test_withdraw_all_non_empty_emits_withdrawn_with_texts() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("aaa", Vec::new()),
        ChatInputEvent::user_message("bbb", Vec::new()),
        ChatInputEvent::WithdrawAll,
    ]);
    let sink = TestSink::default();

    let task_access = task::TaskStore::new();
    task_access
        .create_batch(task::BatchCreateSpec::try_new("request".into()).unwrap(), 1)
        .unwrap();
    task_access
        .create_task(
            task::TaskCreateSpec::try_new(
                "authoritative".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &task_access,
        true, // idle
    )
    .await;

    let texts = sink.events.lock().unwrap().iter().find_map(|e| match e {
        RuntimeStreamEvent::UserMessagesWithdrawn { texts } => Some(texts.clone()),
        _ => None,
    });
    let texts = texts.expect("应发出 UserMessagesWithdrawn");
    assert_eq!(
        texts,
        vec!["aaa".to_string(), "bbb".to_string()],
        "应收集所有 UserMessage text（含已 append 的）"
    );
    assert!(
        outcome.adopted_messages.is_empty(),
        "WithdrawAll 应回滚已 adopt 的消息"
    );
    assert!(buffer.is_empty(), "buffer 应为空");
    assert_eq!(outcome.appended_user_messages, 0);
}

/// #391 S3-3：WithdrawAll 空（无 UserMessage）→ no-op（无事件）。
#[tokio::test]
async fn test_withdraw_all_empty_buffer_is_noop() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::WithdrawAll]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &task::TaskStore::new(),
        true,
    )
    .await;

    let has_withdrawn = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, RuntimeStreamEvent::UserMessagesWithdrawn { .. }));
    assert!(!has_withdrawn, "无 UserMessage 时不应发 Withdrawn");
    assert_eq!(outcome.appended_user_messages, 0);
    assert!(outcome.adopted_messages.is_empty());
}

/// #391 S3-3：busy gate 也立即处理 WithdrawAll（回滚 + 收集，不延迟）。
#[tokio::test]
async fn test_busy_gate_withdraw_all_executes_immediately() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![
        ChatInputEvent::user_message("queued", Vec::new()),
        ChatInputEvent::WithdrawAll,
    ]);
    let sink = TestSink::default();

    let outcome = run_loop_gate(
        GateKind::BeforeLlm,
        &mut buffer,
        &EmptyQueueDrainPort,
        &input,
        &sink,
        &task::TaskStore::new(),
        false, // busy
    )
    .await;

    let texts = sink.events.lock().unwrap().iter().find_map(|e| match e {
        RuntimeStreamEvent::UserMessagesWithdrawn { texts } => Some(texts.clone()),
        _ => None,
    });
    assert!(texts.is_some(), "busy gate 也应处理 WithdrawAll");
    assert_eq!(
        texts.unwrap(),
        vec!["queued".to_string()],
        "只撤回本批 UserMessage"
    );
    // queued 被回滚，无 adopted 消息
    assert!(
        outcome.adopted_messages.is_empty(),
        "queued 应被回滚，无 adopted 消息"
    );
    assert_eq!(outcome.appended_user_messages, 0);
}
