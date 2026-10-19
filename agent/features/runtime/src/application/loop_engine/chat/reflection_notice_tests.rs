//! Scenario tests for the reflection memory-update notice (double channel).
//!
//! A finished reflection that changed memory must reach the TUI at the moment it
//! completes, and leave one LLM reminder behind for the next Run to take. A run
//! without changes, a failed run, and a configuration skip must produce neither.
//!
//! 统一异步链路（#2002）：chat 层 run 派发即 Detached；本文件测 announce 对终态
//! outcome 的映射与 pending notice 归集，走 adapter 测试同步路径注入 executor
//! payload（等价于 detached 完成点拿到的 Completed outcome）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::reflection::{announce_memory_update, memory_updated_notice_text};
use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, EventFuture, RuntimeStreamEvent,
};
use crate::application::reflection::{
    CompleteReflectionResult, ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskRequest,
};

/// Sink that keeps only system messages, which is the whole contract here.
#[derive(Clone, Default)]
struct SystemMessageSink {
    messages: Arc<Mutex<Vec<String>>>,
}

impl SystemMessageSink {
    fn handle(&self) -> ChatEventSinkHandle {
        ChatEventSinkHandle::new(self.clone())
    }

    fn messages(&self) -> Vec<String> {
        self.messages.lock().expect("sink lock").clone()
    }
}

impl ChatEventSink for SystemMessageSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.try_send_event(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        match event {
            RuntimeStreamEvent::SystemMessage(message) => {
                self.messages.lock().expect("sink lock").push(message);
            }
            RuntimeStreamEvent::CommandResultText { text, is_error } if is_error => {
                self.messages.lock().expect("sink lock").push(text);
            }
            _ => {}
        }
    }
}

fn successful_payload() -> CompleteReflectionResult {
    CompleteReflectionResult {
        output: memory::api::reflection::ReflectionOutput::default(),
        input_tokens: 1,
        output_tokens: 1,
        apply_result: None,
        error_category: None,
        record_id: None,
    }
}

fn payload_with(applied: usize) -> CompleteReflectionResult {
    CompleteReflectionResult {
        apply_result: Some(memory::api::reflection::ReflectionApplyResult {
            attempted: applied,
            completed: applied,
            suggestions_added: applied,
            outdated_marked: 0,
            superseded: 0,
        }),
        ..successful_payload()
    }
}

fn manual_request() -> ReflectionTaskRequest {
    ReflectionTaskRequest::new(
        crate::application::reflection::ReflectionTaskTrigger::Manual,
        vec![share::message::Message::user(
            "a conversation worth reflecting on",
        )],
    )
}

async fn run_with_changes(applied: usize) -> (ReflectionTaskAdapter, ReflectionRunOutcome) {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), move |_request, _cancel| {
        let result = payload_with(applied);
        async move { Ok(result) }
    });
    let outcome = adapter.run(manual_request()).await;
    (adapter, outcome)
}

#[test]
fn the_notice_text_states_the_count_in_both_languages() {
    assert_eq!(memory_updated_notice_text(3, "zh"), "记忆已更新 3 条");
    assert_eq!(
        memory_updated_notice_text(1, "en"),
        "Memory updated: 1 entry"
    );
    assert_eq!(
        memory_updated_notice_text(2, "en"),
        "Memory updated: 2 entries"
    );
}

#[tokio::test]
async fn a_changed_memory_announces_the_tui_notice_and_arms_one_llm_reminder() {
    let (adapter, outcome) = run_with_changes(2).await;
    let sink = SystemMessageSink::default();

    let announced = announce_memory_update(&sink.handle(), &outcome, "en").await;

    assert_eq!(announced, Some(2), "the run changed two entries");
    assert_eq!(sink.messages(), ["Memory updated: 2 entries"]);
    assert_eq!(
        adapter.take_memory_update_notice(),
        Some(crate::application::reflection::MemoryUpdateNotice { changed: 2 }),
        "the next Run must find exactly one pending reminder"
    );
    assert_eq!(
        adapter.take_memory_update_notice(),
        None,
        "the reminder must not be injected twice"
    );
}

#[tokio::test]
async fn a_run_without_changes_stays_silent_on_both_channels() {
    let (adapter, outcome) = run_with_changes(0).await;
    let sink = SystemMessageSink::default();

    assert_eq!(
        announce_memory_update(&sink.handle(), &outcome, "en").await,
        None
    );
    assert!(sink.messages().is_empty());
    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn a_disabled_configuration_stays_silent_on_both_channels() {
    // 配置跳过不产生任何 notice（announce 对 DisabledSkipped 的映射）。
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let outcome = ReflectionRunOutcome::DisabledSkipped;
    let sink = SystemMessageSink::default();

    assert_eq!(
        announce_memory_update(&sink.handle(), &outcome, "en").await,
        None
    );
    assert!(sink.messages().is_empty());
    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn two_runs_before_the_next_one_merge_into_a_single_reminder() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(payload_with(1))
    });
    adapter.run(manual_request()).await;
    adapter.run(manual_request()).await;

    assert_eq!(
        adapter.take_memory_update_notice(),
        Some(crate::application::reflection::MemoryUpdateNotice { changed: 2 }),
    );
}

// ---------------------------------------------------------------------------
// detached 完成点失败通知（#2002 follow-up：异步化后 Manual 失败终态不再静默）
// ---------------------------------------------------------------------------

fn terminal_outcome(
    trigger: crate::application::reflection::ReflectionTaskTrigger,
    status: crate::application::reflection::ReflectionTaskCompletionStatus,
) -> crate::application::reflection::ReflectionRunOutcome {
    crate::application::reflection::ReflectionRunOutcome::Completed(
        crate::application::reflection::ReflectionTaskCompletion {
            trigger,
            status,
            metadata: None,
        },
    )
}

/// Manual 失败/超时终态必须发 SystemMessage 错误通知（对齐同步时代的终态回执）。
#[tokio::test]
async fn manual_failure_terminal_notifies_user_after_detached_completion() {
    for (label, status) in [
        (
            "失败",
            crate::application::reflection::ReflectionTaskCompletionStatus::Failed,
        ),
        (
            "超时",
            crate::application::reflection::ReflectionTaskCompletionStatus::TimedOut,
        ),
    ] {
        let sink = SystemMessageSink::default();
        let hook = super::reflection::detached_completion_hook(
            sink.handle(),
            "zh".to_string(),
            None,
            None,
            None,
            crate::application::reflection::ReflectionTaskTrigger::Manual,
        );
        let outcome = terminal_outcome(
            crate::application::reflection::ReflectionTaskTrigger::Manual,
            status,
        );
        hook(outcome).await;
        let messages = sink.messages();
        assert_eq!(
            messages.len(),
            1,
            "{label} 终态必须恰好通知一次: {messages:?}"
        );
        assert!(
            messages[0].contains("Reflection"),
            "{label} 通知必须使用既有终态文案: {:?}",
            messages[0]
        );
    }
}

/// 自动反思（Interval/PreCompact）失败保持静默（不打扰主对话，与同步时代一致）。
#[tokio::test]
async fn automatic_reflection_failure_stays_silent() {
    for trigger in [
        crate::application::reflection::ReflectionTaskTrigger::Interval { run_ordinal: 1 },
        crate::application::reflection::ReflectionTaskTrigger::PreCompact,
    ] {
        let sink = SystemMessageSink::default();
        let hook = super::reflection::detached_completion_hook(
            sink.handle(),
            "zh".to_string(),
            None,
            None,
            None,
            trigger,
        );
        hook(terminal_outcome(
            trigger,
            crate::application::reflection::ReflectionTaskCompletionStatus::Failed,
        ))
        .await;
        assert!(
            sink.messages().is_empty(),
            "自动反思失败必须保持静默: {:?}",
            sink.messages()
        );
    }
}
