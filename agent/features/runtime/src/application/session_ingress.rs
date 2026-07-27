//! Session ingress: the single runtime-facing input boundary.

use std::sync::Arc;

use sdk::{
    ChatInputEvent, InteractionCancelReason, InteractionCommandOutcome, InteractionReply,
    InteractionRequestId,
};

use crate::application::interaction::{InteractionBridge, InteractionPort};

/// Classified input accepted by the session boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIngressCommand {
    UserMessage(ChatInputEvent),
    Command(ChatInputEvent),
    InteractionCommand(InteractionCommand),
}

/// A reply/cancel command addressed to one pending interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionCommand {
    Reply {
        request_id: InteractionRequestId,
        reply: InteractionReply,
    },
    Cancel {
        request_id: InteractionRequestId,
        reason: InteractionCancelReason,
    },
}

/// Runtime's single inbound classification boundary.
pub struct SessionIngress {
    interaction: Arc<InteractionBridge>,
}

/// Mailboxes selected by the session ingress after classification.
pub trait SessionMailboxPort: Send + Sync {
    fn submit_user_message(&self, event: ChatInputEvent);
    fn schedule_command(&self, event: ChatInputEvent);
    fn dispatch_interaction(&self, command: InteractionCommand) -> InteractionCommandOutcome;
}

impl SessionIngress {
    pub(crate) fn new(interaction: Arc<InteractionBridge>) -> Self {
        Self { interaction }
    }

    pub fn classify(event: ChatInputEvent) -> SessionIngressCommand {
        match event {
            ChatInputEvent::UserMessage { .. } => SessionIngressCommand::UserMessage(event),
            _ => SessionIngressCommand::Command(event),
        }
    }

    pub fn route(
        event: SessionIngressCommand,
        mailboxes: &dyn SessionMailboxPort,
    ) -> Option<InteractionCommandOutcome> {
        match event {
            SessionIngressCommand::UserMessage(event) => {
                mailboxes.submit_user_message(event);
                None
            }
            SessionIngressCommand::Command(event) => {
                mailboxes.schedule_command(event);
                None
            }
            SessionIngressCommand::InteractionCommand(command) => {
                Some(mailboxes.dispatch_interaction(command))
            }
        }
    }

    pub(crate) fn dispatch_interaction(
        &self,
        command: InteractionCommand,
    ) -> InteractionCommandOutcome {
        match command {
            InteractionCommand::Reply {
                request_id, reply, ..
            } => self.interaction.reply(&request_id, reply),
            InteractionCommand::Cancel {
                request_id, reason, ..
            } => self.interaction.cancel(&request_id, reason),
        }
    }
}
