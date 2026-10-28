//! idle 生命周期的 wakeup 接线测试（#252 PR2）+ `/think` 档位执行测试（#2032）。

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::idle_lifecycle::{execute_set_thinking, idle_until_resume_or_shutdown, IdleResult};
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
    let result = idle_until_resume_or_shutdown(
        &source,
        &sink,
        &mut pending,
        &task_store,
        Some(&mut waiter),
        &crate::application::loop_engine::chat::immediate_query::test_support::no_io_dispatcher(),
    )
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
        &crate::application::loop_engine::chat::immediate_query::test_support::no_io_dispatcher(),
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

#[derive(Default, Clone)]
struct RecordingSink(Arc<Mutex<Vec<&'static str>>>);

impl RecordingSink {
    fn kinds(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn record(&self, event: &RuntimeStreamEvent) {
        let kind = match event {
            RuntimeStreamEvent::ModelList { .. } => "ModelList",
            RuntimeStreamEvent::CommandResultText { .. } => "CommandResultText",
            _ => "Other",
        };
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(kind);
    }
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(
        &'a self,
        event: RuntimeStreamEvent,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        self.record(&event);
        Box::pin(async {})
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.record(&event);
    }
}

#[tokio::test]
async fn idle_dispatches_list_models_immediately_without_queueing() {
    // idle 收到查询后必须就地执行并回 ModelList，NEVER 进入 pending 队列。
    let source = ScriptedInputSource {
        received: Arc::new(Mutex::new(VecDeque::from(vec![
            ChatInputEvent::ListModels,
            user_event("hello"),
        ]))),
    };
    let mut pending = PendingInputBuffer::default();
    let sink = RecordingSink::default();

    let result = idle_until_resume_or_shutdown(
        &source,
        &sink,
        &mut pending,
        &task::TaskStore::new(),
        None,
        &crate::application::loop_engine::chat::immediate_query::test_support::no_io_dispatcher(),
    )
    .await;

    let has_model_list = sink.kinds().contains(&"ModelList");
    assert!(
        has_model_list,
        "idle 必须即时回 ModelList，实际事件: {:?}",
        sink.kinds()
    );
    match result {
        IdleResult::Resumed {
            accepted_inputs, ..
        } => assert_eq!(accepted_inputs.len(), 1, "仅用户消息被受理"),
        other => panic!("查询后应继续等待用户输入，实际 {other:?}"),
    }
}

const fn _static_assert_sink_clone_send_sync()
where
    NullSink: Clone + Send + Sync + 'static,
{
}

// ── #2032：/think 档位执行 ──

/// 记录 ThinkingChanged 明细与 SystemMessage 回执的 sink。
#[derive(Default, Clone)]
struct ThinkingRecorderSink {
    changed: Arc<
        Mutex<
            Vec<(
                bool,
                share::reasoning::ReasoningLevel,
                share::reasoning::ReasoningLevel,
            )>,
        >,
    >,
    receipts: Arc<Mutex<Vec<String>>>,
}

impl ChatEventSink for ThinkingRecorderSink {
    fn send_event<'a>(
        &'a self,
        event: RuntimeStreamEvent,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        match event {
            RuntimeStreamEvent::ThinkingChanged {
                enabled,
                requested,
                effective,
            } => {
                self.changed
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push((enabled, requested, effective));
            }
            RuntimeStreamEvent::SystemMessage(text) => {
                self.receipts
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(text);
            }
            _ => {}
        }
        Box::pin(async {})
    }

    fn try_send_event(&self, _event: RuntimeStreamEvent) {}
}

/// 构造支持阶梯 ≤ `max_level` 的测试 binding。
fn thinking_binding(
    max_level: share::reasoning::ReasoningLevel,
) -> crate::ports::ProviderBindingData {
    let ladder = [
        share::reasoning::ReasoningLevel::Off,
        share::reasoning::ReasoningLevel::Minimal,
        share::reasoning::ReasoningLevel::Low,
        share::reasoning::ReasoningLevel::Medium,
        share::reasoning::ReasoningLevel::High,
        share::reasoning::ReasoningLevel::Xhigh,
        share::reasoning::ReasoningLevel::Max,
    ];
    let model = crate::ports::provider_port::ModelInfo {
        provider: "test".to_string(),
        model: "test-model".to_string(),
        supports_tools: true,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        supported_reasoning: ladder
            .into_iter()
            .filter(|level| *level <= max_level)
            .collect(),
        server_effort_map: Vec::new(),
        context_limit: Some(128_000),
        output_limit: Some(8_192),
    };
    crate::ports::ProviderBindingData {
        provider: Arc::new(crate::ports::provider_port::fake::FakeProvider::new()),
        model,
        max_tokens: 4_096,
        requested_reasoning: share::reasoning::ReasoningLevel::Medium,
    }
}

#[tokio::test]
async fn set_thinking_level_emits_requested_and_clamped_effective() {
    let control = Mutex::new(
        crate::application::run::session_reasoning::SessionReasoningControl::from_model_default(
            share::reasoning::ReasoningLevel::Medium,
        ),
    );
    let sink = ThinkingRecorderSink::default();
    // 模型能力上限 High：请求 Max 应 clamp 到 High。
    let binding = thinking_binding(share::reasoning::ReasoningLevel::High);

    let requested = execute_set_thinking(
        &control,
        &sink,
        sdk::ThinkingRequest::Level(share::reasoning::ReasoningLevel::Max),
        &binding,
    )
    .await;

    assert_eq!(requested, share::reasoning::ReasoningLevel::Max);
    assert_eq!(
        sink.changed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_slice(),
        [(
            true,
            share::reasoning::ReasoningLevel::Max,
            share::reasoning::ReasoningLevel::High
        )],
        "ThinkingChanged 应携带请求档位与 clamp 后映射档位"
    );
    let receipts = sink
        .receipts
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    assert_eq!(receipts.len(), 1);
    assert!(
        receipts[0].contains("high") && receipts[0].contains("clamped"),
        "回执应展示映射档位与 clamp 说明，实际：{}",
        receipts[0]
    );
}

#[tokio::test]
async fn set_thinking_level_within_capability_has_plain_receipt() {
    let control = Mutex::new(
        crate::application::run::session_reasoning::SessionReasoningControl::from_model_default(
            share::reasoning::ReasoningLevel::Off,
        ),
    );
    let sink = ThinkingRecorderSink::default();
    let binding = thinking_binding(share::reasoning::ReasoningLevel::Max);

    let requested = execute_set_thinking(
        &control,
        &sink,
        sdk::ThinkingRequest::Level(share::reasoning::ReasoningLevel::High),
        &binding,
    )
    .await;

    assert_eq!(requested, share::reasoning::ReasoningLevel::High);
    let receipts = sink
        .receipts
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0], "[thinking mode: high]");
}

#[tokio::test]
async fn set_thinking_toggle_off_then_on_roundtrips() {
    let control = Mutex::new(
        crate::application::run::session_reasoning::SessionReasoningControl::from_user_selection(
            share::reasoning::ReasoningLevel::Xhigh,
        ),
    );
    let sink = ThinkingRecorderSink::default();
    let binding = thinking_binding(share::reasoning::ReasoningLevel::Max);

    execute_set_thinking(&control, &sink, sdk::ThinkingRequest::Toggle, &binding).await;
    execute_set_thinking(&control, &sink, sdk::ThinkingRequest::Toggle, &binding).await;

    let changed = sink
        .changed
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    assert_eq!(
        changed.as_slice(),
        [
            (
                false,
                share::reasoning::ReasoningLevel::Off,
                share::reasoning::ReasoningLevel::Off
            ),
            (
                true,
                share::reasoning::ReasoningLevel::Medium,
                share::reasoning::ReasoningLevel::Medium
            ),
        ],
        "toggle 应先关（Off）再开（Medium），与旧语义一致"
    );
}
