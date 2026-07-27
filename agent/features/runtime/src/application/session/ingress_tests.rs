use std::sync::Arc;

use sdk::{
    ChatInputEvent, InteractionReply, InteractionRequest, InteractionRequestBody, RunId,
    StuckDiagnostic,
};

use super::{InteractionCommand, SessionIngress, SessionIngressCommand, SessionMailboxPort};
use crate::application::interaction::port::InteractionPort;

#[test]
fn user_message_is_classified_for_run_input() {
    let event = ChatInputEvent::user_message("hello", Vec::new());
    assert!(matches!(
        SessionIngress::classify(event),
        SessionIngressCommand::UserMessage(ChatInputEvent::UserMessage { .. })
    ));
}

#[test]
fn control_event_is_classified_as_command() {
    let event = ChatInputEvent::Reset;
    assert!(matches!(
        SessionIngress::classify(event),
        SessionIngressCommand::Command(ChatInputEvent::Reset)
    ));
}

#[derive(Default)]
struct RecordingMailboxes {
    user_messages: std::sync::Mutex<Vec<ChatInputEvent>>,
    commands: std::sync::Mutex<Vec<ChatInputEvent>>,
}

impl SessionMailboxPort for RecordingMailboxes {
    fn submit_user_message(&self, event: ChatInputEvent) {
        self.user_messages.lock().unwrap().push(event);
    }

    fn schedule_command(&self, event: ChatInputEvent) {
        self.commands.lock().unwrap().push(event);
    }

    fn dispatch_interaction(&self, _command: InteractionCommand) -> sdk::InteractionCommandOutcome {
        sdk::InteractionCommandOutcome::Accepted
    }
}

#[test]
fn classified_events_reach_distinct_mailboxes() {
    let mailboxes = RecordingMailboxes::default();
    SessionIngress::route(
        SessionIngress::classify(ChatInputEvent::user_message("hello", Vec::new())),
        &mailboxes,
    );
    SessionIngress::route(SessionIngress::classify(ChatInputEvent::Reset), &mailboxes);
    assert_eq!(mailboxes.user_messages.lock().unwrap().len(), 1);
    assert_eq!(mailboxes.commands.lock().unwrap().len(), 1);
}

#[test]
fn interaction_command_is_dispatched_through_ingress() {
    let bridge = Arc::new(crate::application::interaction::port::InteractionBridge::new());
    let ingress = SessionIngress::new(bridge.clone());
    let request = InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: RunId::new_v7(),
        body: InteractionRequestBody::HardPause(StuckDiagnostic {
            reason: "test".to_string(),
            recent_actions: Vec::new(),
        }),
    };
    let _receiver = bridge
        .register(request.clone())
        .expect("register interaction");
    let outcome = ingress.dispatch_interaction(InteractionCommand::Reply {
        request_id: request.id,
        reply: InteractionReply::HardPauseContinue,
    });
    assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);
}
