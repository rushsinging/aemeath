use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use sdk::{
    ChatInputEvent, ChatInputEventPort, InputEventFuture, InputEventOptFuture, InteractionReply,
    InteractionRequest, InteractionRequestBody, RunId, StuckDiagnostic,
};

use super::{InteractionCommand, SessionIngress, SessionInputMailbox};
use crate::application::interaction::port::InteractionPort;

#[derive(Default)]
struct ScriptedInputSource {
    available: Mutex<VecDeque<ChatInputEvent>>,
    received: Mutex<VecDeque<Option<ChatInputEvent>>>,
}

impl ScriptedInputSource {
    fn with_available(events: impl IntoIterator<Item = ChatInputEvent>) -> Self {
        Self {
            available: Mutex::new(events.into_iter().collect()),
            received: Mutex::new(VecDeque::new()),
        }
    }

    fn with_received(events: impl IntoIterator<Item = Option<ChatInputEvent>>) -> Self {
        Self {
            available: Mutex::new(VecDeque::new()),
            received: Mutex::new(events.into_iter().collect()),
        }
    }
}

impl ChatInputEventPort for ScriptedInputSource {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move {
            self.available
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .drain(..)
                .collect()
        })
    }

    fn recv_next<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move {
            self.received
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .pop_front()
                .flatten()
        })
    }
}

fn user_event(text: &str) -> ChatInputEvent {
    ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: text.to_string(),
        images: Vec::new(),
    }
}

#[tokio::test]
async fn input_mailbox_receives_deferred_event_before_source() {
    let source = Arc::new(ScriptedInputSource::with_received([Some(user_event(
        "source",
    ))]));
    let mailbox = SessionInputMailbox::new(source);
    mailbox.defer(ChatInputEvent::Compact);

    assert!(matches!(
        mailbox.recv_next().await,
        Some(ChatInputEvent::Compact)
    ));
    assert!(matches!(
        mailbox.recv_next().await,
        Some(ChatInputEvent::UserMessage { text, .. }) if text == "source"
    ));
}

#[tokio::test]
async fn input_mailbox_drains_deferred_then_available_in_fifo_order() {
    let source = Arc::new(ScriptedInputSource::with_available([
        user_event("source-a"),
        user_event("source-b"),
    ]));
    let mailbox = SessionInputMailbox::new(source);
    mailbox.defer(user_event("deferred"));

    let events = mailbox.drain_available().await;
    let texts: Vec<_> = events
        .into_iter()
        .filter_map(|event| match event {
            ChatInputEvent::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect();

    assert_eq!(texts, ["deferred", "source-a", "source-b"]);
}

#[tokio::test]
async fn input_mailbox_remembers_source_close_without_repolling() {
    let source = Arc::new(ScriptedInputSource::with_received([None]));
    let mailbox = SessionInputMailbox::new(source);

    assert!(mailbox.recv_next().await.is_none());
    assert!(mailbox.is_source_closed());
    assert!(mailbox.recv_next().await.is_none());
}

#[test]
fn interaction_command_is_dispatched_through_ingress() {
    let bridge = Arc::new(crate::application::interaction::port::InteractionBridge::new());
    let ingress = SessionIngress::new(bridge.clone());
    let request = InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: RunId::new_v7(),
        tool_call_id: None,
        body: InteractionRequestBody::HardPause(StuckDiagnostic {
            reason: "test".to_string(),
            recent_actions: Vec::new(),
        }),
    };
    let mut receiver = bridge
        .register(request.clone())
        .expect("register interaction");
    let outcome = ingress.dispatch_interaction(InteractionCommand::Reply {
        request_id: request.id,
        reply: InteractionReply::HardPauseContinue,
    });
    assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);
    assert!(matches!(
        receiver.try_recv(),
        Ok(
            crate::application::interaction::port::InteractionCompletion::Replied(
                InteractionReply::HardPauseContinue
            )
        )
    ));
}
