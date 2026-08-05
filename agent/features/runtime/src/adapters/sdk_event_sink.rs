use sdk::{ChangeSet, ChatEvent};

use crate::application::loop_engine::chat::ChatEventSink;

use crate::adapters::sdk_event_mapper::{map_activity_event, map_stream_event};

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

    fn project_and_mark(
        &self,
        event: crate::application::loop_engine::chat::RuntimeStreamEvent,
    ) -> ChatEvent {
        let projected = map_stream_event(event);
        if matches!(projected, ChatEvent::WorkingDirectoryChanged { .. }) {
            let previous = *self.change_tx.borrow();
            self.change_tx.send_replace(previous | ChangeSet::PROJECT);
        }
        projected
    }
}

impl crate::application::activity::ActivityChangePublisher
    for crate::application::loop_engine::chat::ChatEventSinkHandle
{
    fn publish_change(&self, kind: sdk::ActivityChangeKind, activity: sdk::ActivityView) {
        self.send_activity_event(
            crate::application::loop_engine::chat::RuntimeActivityEvent::Changed {
                kind,
                activity: Box::new(activity),
            },
        );
    }

    fn publish_snapshot(&self, snapshot: sdk::ActivitySnapshotView) {
        self.send_activity_event(
            crate::application::loop_engine::chat::RuntimeActivityEvent::Snapshot(snapshot),
        );
    }
}

impl crate::application::loop_engine::chat::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::application::loop_engine::chat::RuntimeStreamEvent,
    ) -> crate::application::loop_engine::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self.tx.send(self.project_and_mark(event));
        })
    }

    fn try_send_event(&self, event: crate::application::loop_engine::chat::RuntimeStreamEvent) {
        let _ = self.tx.send(self.project_and_mark(event));
    }

    fn send_activity_event(
        &self,
        event: crate::application::loop_engine::chat::RuntimeActivityEvent,
    ) {
        let _ = self.tx.send(map_activity_event(event));
    }

    fn send_lifecycle_event<'a>(
        &'a self,
        event: crate::domain::agent_run::RuntimeLifecycleEvent,
    ) -> crate::application::loop_engine::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self
                .tx
                .send(crate::adapters::sdk_event_mapper::map_lifecycle_event(
                    event,
                ));
        })
    }
}
