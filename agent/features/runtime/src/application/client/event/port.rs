use std::sync::Arc;

use sdk::{ChangeSet, ChatEvent};

use super::convert::runtime_event_to_sdk_event;

#[derive(Clone)]
pub(crate) struct SdkChatEventSink {
    pub(crate) tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
    pub(crate) change_tx: tokio::sync::watch::Sender<ChangeSet>,
}

impl crate::application::chat::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::application::chat::RuntimeStreamEvent,
    ) -> crate::application::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self
                .tx
                .send(runtime_event_to_sdk_event(event, &self.change_tx));
        })
    }

    fn try_send_event(&self, event: crate::application::chat::RuntimeStreamEvent) {
        let _ = self
            .tx
            .send(runtime_event_to_sdk_event(event, &self.change_tx));
    }
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeQueueDrainPort {
    inner: Option<Arc<dyn sdk::QueueDrainPort>>,
}

impl RuntimeQueueDrainPort {
    pub(crate) fn new(inner: Option<Arc<dyn sdk::QueueDrainPort>>) -> Self {
        Self { inner }
    }
}

impl crate::application::chat::QueueDrainPort for RuntimeQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::application::chat::QueueFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.drain_queued_input().await,
                None => None,
            }
        })
    }
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeInputEventDrainPort {
    inner: Option<Arc<dyn sdk::ChatInputEventPort>>,
}

impl RuntimeInputEventDrainPort {
    pub(crate) fn new(inner: Option<Arc<dyn sdk::ChatInputEventPort>>) -> Self {
        Self { inner }
    }
}

impl crate::application::chat::InputEventDrainPort for RuntimeInputEventDrainPort {
    fn drain_input_events<'a>(&'a self) -> crate::application::chat::InputEventFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.drain_input_events().await,
                None => Vec::new(),
            }
        })
    }

    fn recv_next_input<'a>(&'a self) -> crate::application::chat::InputEventOptFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(port) => port.recv_next().await,
                None => None,
            }
        })
    }
}
