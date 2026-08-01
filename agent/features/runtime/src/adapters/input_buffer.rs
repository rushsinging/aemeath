//! Concrete typed input adapter — wraps the SDK ingress as a Session-owned mailbox.

use crate::application::loop_engine::chat::{
    InputEventDrainPort, InputEventFuture, InputEventOptFuture,
};
use crate::application::session::ingress::SessionInputMailbox;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct RuntimeInputEventDrainPort {
    mailbox: SessionInputMailbox,
}

impl RuntimeInputEventDrainPort {
    pub(crate) fn new(ingress: Arc<dyn sdk::ChatInputEventPort>) -> Self {
        Self {
            mailbox: SessionInputMailbox::new(ingress),
        }
    }
}

impl crate::application::loop_engine::input_strategy::SessionInputPort
    for RuntimeInputEventDrainPort
{
    fn defer(&self, event: sdk::ChatInputEvent) {
        self.mailbox.defer(event);
    }
}

impl InputEventDrainPort for RuntimeInputEventDrainPort {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async move { self.mailbox.drain_available().await })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async move { self.mailbox.recv_next().await })
    }
}
