use crate::business::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use crate::business::chat::looping::queue::{QueueDrainPort, QueueFuture};
use sdk::ChatInputEvent;
use share::message::Message;
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;

pub type InputEventFuture<'a> = Pin<Box<dyn Future<Output = Vec<ChatInputEvent>> + Send + 'a>>;

pub trait InputEventDrainPort: Clone + Send + Sync + 'static {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    BeforeLlm,
    BeforeFinish,
    AfterBlockingBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Proceed,
    ContinueNextTurn,
    AbortCurrentLoop,
    CancelCurrentLoop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCommandKind {
    Abort,
    SideEffect,
    Reconfigure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCommand {
    pub raw: String,
    pub kind: ControlCommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateOutcome {
    pub decision: GateDecision,
    pub commands: Vec<ControlCommand>,
    pub appended_user_messages: usize,
    pub dropped_events: usize,
}

#[derive(Debug, Clone, Default)]
pub struct PendingInputBuffer {
    events: VecDeque<ChatInputEvent>,
    seen_user_messages: HashSet<(String, Vec<String>)>,
}

impl PendingInputBuffer {
    pub fn push(&mut self, event: ChatInputEvent) {
        if self.should_accept(&event) {
            self.events.push_back(event);
        }
    }

    pub fn extend(&mut self, events: impl IntoIterator<Item = ChatInputEvent>) {
        for event in events {
            self.push(event);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    fn should_accept(&mut self, event: &ChatInputEvent) -> bool {
        match event {
            ChatInputEvent::UserMessage { text, image_paths } => self
                .seen_user_messages
                .insert((text.clone(), image_paths.clone())),
            ChatInputEvent::ControlCommand { .. } | ChatInputEvent::Cancel => true,
        }
    }

    fn drain(&mut self) -> Vec<ChatInputEvent> {
        self.events.drain(..).collect()
    }
}

pub async fn run_loop_gate<Q, I, S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    queue: &Q,
    input_events: &I,
    sink: &S,
    messages: &mut Vec<Message>,
) -> GateOutcome
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    drain_sources(buffer, queue, input_events).await;
    apply_gate(kind, buffer, sink, messages).await
}

pub async fn drain_sources<Q, I>(buffer: &mut PendingInputBuffer, queue: &Q, input_events: &I)
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    buffer.extend(input_events.drain_input_events().await);
    if let Some(queued) = queue.drain_queued_input().await {
        buffer.extend(
            queued
                .into_iter()
                .map(|text| ChatInputEvent::classify_text(text, Vec::new())),
        );
    }
}

pub async fn apply_gate<S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    sink: &S,
    messages: &mut Vec<Message>,
) -> GateOutcome
where
    S: ChatEventSink,
{
    let mut commands = Vec::new();
    let mut appended_user_messages = 0usize;
    let mut dropped_events = 0usize;
    let mut decision = GateDecision::Proceed;

    let events = buffer.drain();
    let mut appended_this_gate = Vec::new();
    let mut iter = events.into_iter().peekable();
    while let Some(event) = iter.next() {
        match event {
            ChatInputEvent::Cancel => {
                decision = GateDecision::CancelCurrentLoop;
                break;
            }
            ChatInputEvent::ControlCommand { raw } => {
                let kind = classify_control_command(&raw);
                commands.push(ControlCommand {
                    raw: raw.clone(),
                    kind: kind.clone(),
                });
                if kind == ControlCommandKind::Abort {
                    dropped_events = iter.count();
                    for _ in 0..appended_this_gate.len() {
                        messages.pop();
                    }
                    appended_user_messages = 0;
                    decision = GateDecision::AbortCurrentLoop;
                    break;
                }
            }
            ChatInputEvent::UserMessage { text, image_paths } => {
                logging::UnifiedLogger::log_user_input(serde_json::json!({
                    "text": &text,
                    "image_paths": &image_paths,
                }));
                messages.push(user_message_with_images(text, image_paths));
                appended_this_gate.push(());
                appended_user_messages += 1;
            }
        }
    }

    if appended_user_messages > 0 {
        sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
            .await;
    }

    if decision == GateDecision::Proceed && appended_user_messages > 0 {
        decision = match kind {
            GateKind::AfterBlockingBoundary => GateDecision::Proceed,
            GateKind::BeforeLlm | GateKind::BeforeFinish => GateDecision::ContinueNextTurn,
        };
    }

    GateOutcome {
        decision,
        commands,
        appended_user_messages,
        dropped_events,
    }
}

fn classify_control_command(raw: &str) -> ControlCommandKind {
    let command = raw.split_whitespace().next().unwrap_or_default();
    match command {
        "/clear" => ControlCommandKind::Abort,
        "/model" | "/provider" => ControlCommandKind::Reconfigure,
        _ => ControlCommandKind::SideEffect,
    }
}

fn user_message_with_images(text: String, image_paths: Vec<String>) -> Message {
    if image_paths.is_empty() {
        return Message::user(text);
    }

    log::warn!(
        "queued ChatInputEvent image_paths are not decoded in runtime gate yet; appending text only (image_count={})",
        image_paths.len()
    );
    Message::user(text)
}

#[derive(Clone, Default)]
pub struct EmptyInputEventDrainPort;

impl InputEventDrainPort for EmptyInputEventDrainPort {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async { Vec::new() })
    }
}

#[derive(Clone, Default)]
pub struct EmptyQueueDrainPort;

impl QueueDrainPort for EmptyQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
        Box::pin(async { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::chat::looping::events::EventFuture;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestInputEventPort {
        events: Arc<Mutex<Vec<ChatInputEvent>>>,
    }

    impl TestInputEventPort {
        fn new(events: Vec<ChatInputEvent>) -> Self {
            Self {
                events: Arc::new(Mutex::new(events)),
            }
        }
    }

    impl InputEventDrainPort for TestInputEventPort {
        fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
            Box::pin(async move { self.events.lock().unwrap().drain(..).collect() })
        }
    }

    #[derive(Clone)]
    struct TestQueuePort {
        queued: Arc<Mutex<Option<Vec<String>>>>,
    }

    impl TestQueuePort {
        fn new(queued: Option<Vec<String>>) -> Self {
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
    struct TestSink {
        events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
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
        let mut messages = vec![Message::user("first")];

        let outcome = run_loop_gate(
            GateKind::BeforeFinish,
            &mut buffer,
            &queue,
            &input,
            &sink,
            &mut messages,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
        assert_eq!(outcome.appended_user_messages, 1);
        assert_eq!(messages.last().unwrap().text_content(), "继续");
        assert_eq!(sink.events.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_run_loop_gate_after_blocking_appends_without_continue_decision() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
            "tool 后输入",
            Vec::new(),
        )]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let outcome = run_loop_gate(
            GateKind::AfterBlockingBoundary,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::Proceed);
        assert_eq!(outcome.appended_user_messages, 1);
        assert_eq!(messages[0].text_content(), "tool 后输入");
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
        let mut messages = Vec::new();

        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
        assert_eq!(outcome.commands.len(), 1);
        assert_eq!(outcome.commands[0].raw, "/save");
        assert_eq!(messages[0].text_content(), "text1");
        assert_eq!(messages[1].text_content(), "text2");
    }

    #[tokio::test]
    async fn test_run_loop_gate_cancel_overrides_user_message() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::Cancel,
            ChatInputEvent::user_message("ignored", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let outcome = run_loop_gate(
            GateKind::BeforeFinish,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::CancelCurrentLoop);
        assert!(messages.is_empty());
        assert!(sink.events.lock().unwrap().is_empty());
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
        let mut messages = Vec::new();

        let outcome = run_loop_gate(
            GateKind::BeforeFinish,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::AbortCurrentLoop);
        assert_eq!(outcome.dropped_events, 1);
        assert_eq!(outcome.commands[0].kind, ControlCommandKind::Abort);
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_run_loop_gate_deduplicates_push_and_pull_user_message() {
        let mut buffer = PendingInputBuffer::default();
        let queue = TestQueuePort::new(Some(vec!["same".to_string()]));
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message("same", Vec::new())]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &queue,
            &input,
            &sink,
            &mut messages,
        )
        .await;

        assert_eq!(outcome.appended_user_messages, 1);
        assert_eq!(messages.len(), 1);
    }
}
