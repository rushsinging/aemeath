//! input_gate 的 Reset / WithdrawAll 语义测试（#391 系列）。
//!
//! 从 `input_gate_tests.rs` 拆出，复用其中的 harness（TestInputEventPort / TestSink 等）。

use super::input_gate_tests::{test_task_store, TestInputEventPort, TestSink};
use crate::business::chat::looping::events::RuntimeStreamEvent;
use crate::business::chat::looping::input_gate::{
    run_loop_gate, EmptyQueueDrainPort, GateDecision, GateKind, PendingInputBuffer,
};
use context::api::session::ChatChain;
use sdk::ChatInputEvent;
use share::message::Message;

/// #391 S1-3：idle gate 收到 Reset → 清空 messages + 发 SessionReset，保持 idle。
#[tokio::test]
async fn test_idle_gate_reset_clears_messages_and_emits_session_reset() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::Reset]);
    let sink = TestSink::default();
    let mut chain =
        ChatChain::from_flat_messages(vec![Message::user("old1"), Message::user("resp1")]);

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
        true, // idle
    )
    .await;

    assert_eq!(outcome.decision, GateDecision::Proceed);
    assert!(
        chain.is_empty(),
        "idle Reset 应清空所有消息，实际 {:?}",
        chain.messages_flat()
    );
    let has_reset = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, RuntimeStreamEvent::SessionReset));
    assert!(has_reset, "应发出 SessionReset 事件");
}

/// #391 S1-3：busy gate 收到 Reset → 放回 buffer，messages 不变，等 idle 处理。
#[tokio::test]
async fn test_busy_gate_reset_defers_to_buffer() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::Reset]);
    let sink = TestSink::default();
    let mut chain = ChatChain::from_flat_messages(vec![Message::user("old1")]);

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
        false, // busy
    )
    .await;

    assert_eq!(
        chain.messages_flat().len(),
        1,
        "busy gate 不应清空 messages"
    );
    assert_eq!(buffer.len(), 1, "Reset 应留在 buffer 等待 idle");
    assert_eq!(outcome.decision, GateDecision::Proceed);
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
    let mut chain = ChatChain::from_flat_messages(vec![Message::user("old1")]);

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
        true, // idle
    )
    .await;

    assert_eq!(outcome.dropped_events, 1, "Reset 后的 UserMessage 应被丢弃");
    assert!(chain.is_empty(), "Reset 清空后不应 append 后续消息");
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
    assert!(chain.is_empty(), "WithdrawAll 应回滚已 append 的消息");
    assert!(buffer.is_empty(), "buffer 应为空");
    assert_eq!(outcome.appended_user_messages, 0);
}

/// #391 S3-3：WithdrawAll 空（无 UserMessage）→ no-op（无事件）。
#[tokio::test]
async fn test_withdraw_all_empty_buffer_is_noop() {
    let mut buffer = PendingInputBuffer::default();
    let input = TestInputEventPort::new(vec![ChatInputEvent::WithdrawAll]);
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
    let mut chain = ChatChain::from_flat_messages(vec![Message::user("existing")]);

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
    // existing 保留（上一回合的），queued 被回滚
    assert_eq!(
        chain.messages_flat().len(),
        1,
        "queued 应被回滚，只保留 existing"
    );
    assert_eq!(chain.messages_flat()[0].text_content(), "existing");
    assert_eq!(outcome.appended_user_messages, 0);
}
