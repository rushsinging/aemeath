//! Session ingress: the single runtime-facing input boundary.

use std::sync::Arc;

use sdk::{
    InteractionCancelReason, InteractionCommandOutcome, InteractionReply, InteractionRequestId,
};

use crate::application::interaction::port::{InteractionBridge, InteractionPort};

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

impl SessionIngress {
    pub(crate) fn new(interaction: Arc<InteractionBridge>) -> Self {
        Self { interaction }
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

#[cfg(test)]
#[path = "ingress_tests.rs"]
mod tests;
