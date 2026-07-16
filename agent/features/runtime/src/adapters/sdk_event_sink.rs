use sdk::{ChangeSet, ChatEvent};

use crate::adapters::event_projection::project_stream_event;

#[derive(Clone)]
pub struct SdkChatEventSink {
    tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
    change_tx: tokio::sync::watch::Sender<ChangeSet>,
}

impl SdkChatEventSink {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>) -> Self {
        let (change_tx, _) = tokio::sync::watch::channel(ChangeSet::empty());
        Self { tx, change_tx }
    }

    fn project_and_mark(&self, event: crate::application::chat::RuntimeStreamEvent) -> ChatEvent {
        let projected = project_stream_event(event);
        if matches!(projected, ChatEvent::WorkingDirectoryChanged { .. }) {
            let previous = *self.change_tx.borrow();
            self.change_tx.send_replace(previous | ChangeSet::PROJECT);
        }
        projected
    }
}

impl crate::application::chat::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::application::chat::RuntimeStreamEvent,
    ) -> crate::application::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self.tx.send(self.project_and_mark(event));
        })
    }

    fn try_send_event(&self, event: crate::application::chat::RuntimeStreamEvent) {
        let _ = self.tx.send(self.project_and_mark(event));
    }

    fn send_domain_event<'a>(
        &'a self,
        event: crate::domain::agent_run::RunDomainEvent,
    ) -> crate::application::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self
                .tx
                .send(crate::adapters::event_projection::project_domain_event(
                    event,
                ));
        })
    }
}
