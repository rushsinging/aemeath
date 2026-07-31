//! Session ingress: the single runtime-facing input boundary.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use sdk::{
    ChatInputEvent, ChatInputEventPort, InteractionCancelReason, InteractionCommandOutcome,
    InteractionReply, InteractionRequestId,
};

use crate::application::interaction::port::{InteractionBridge, InteractionPort};

/// Session-owned typed input mailbox.
///
/// This is the only Runtime type allowed to poll the SDK input source. Events
/// rejected by a sealed Run are deferred here and observed before newer source
/// events, preserving producer identity and FIFO order across Run boundaries.
#[derive(Clone)]
pub(crate) struct SessionInputMailbox {
    source: Arc<dyn ChatInputEventPort>,
    state: Arc<Mutex<SessionInputMailboxState>>,
}

#[derive(Default)]
struct SessionInputMailboxState {
    deferred: VecDeque<ChatInputEvent>,
    source_closed: bool,
}

impl SessionInputMailbox {
    pub(crate) fn new(source: Arc<dyn ChatInputEventPort>) -> Self {
        Self {
            source,
            state: Arc::new(Mutex::new(SessionInputMailboxState::default())),
        }
    }

    pub(crate) fn defer(&self, event: ChatInputEvent) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .deferred
            .push_back(event);
    }

    pub(crate) async fn drain_available(&self) -> Vec<ChatInputEvent> {
        let mut events: Vec<_> = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .deferred
            .drain(..)
            .collect();
        if !self.is_source_closed() {
            events.extend(self.source.drain_input_events().await);
        }
        events
    }

    pub(crate) async fn recv_next(&self) -> Option<ChatInputEvent> {
        if let Some(event) = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .deferred
            .pop_front()
        {
            return Some(event);
        }
        if self.is_source_closed() {
            return None;
        }
        let event = self.source.recv_next().await;
        if event.is_none() {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .source_closed = true;
        }
        event
    }

    pub(crate) fn is_source_closed(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .source_closed
    }
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

impl SessionIngress {
    pub(crate) fn new(interaction: Arc<InteractionBridge>) -> Self {
        Self { interaction }
    }

    pub(crate) fn dispatch_interaction(
        &self,
        command: InteractionCommand,
    ) -> InteractionCommandOutcome {
        let (operation, request_id, outcome) = match command {
            InteractionCommand::Reply {
                request_id, reply, ..
            } => {
                let outcome = self.interaction.reply(&request_id, reply);
                ("reply", request_id, outcome)
            }
            InteractionCommand::Cancel {
                request_id, reason, ..
            } => {
                let outcome = self.interaction.cancel(&request_id, reason);
                ("cancel", request_id, outcome)
            }
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "session ingress dispatched interaction: operation={operation} request_id={} outcome={outcome:?}",
            request_id.as_str()
        );
        outcome
    }
}

#[cfg(test)]
#[path = "ingress_tests.rs"]
mod tests;
