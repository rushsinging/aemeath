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

    fn project_and_mark(
        &self,
        event: crate::application::main_loop::RuntimeStreamEvent,
    ) -> ChatEvent {
        let projected = project_stream_event(event);
        if matches!(projected, ChatEvent::WorkingDirectoryChanged { .. }) {
            let previous = *self.change_tx.borrow();
            self.change_tx.send_replace(previous | ChangeSet::PROJECT);
        }
        projected
    }

    fn send_projected(&self, projected: ChatEvent, mode: &'static str) {
        let diagnostic = match &projected {
            ChatEvent::Token { context, text } => Some((
                "Text",
                Some(context.chat_id.as_str().to_owned()),
                Some(context.turn_id.as_str().to_owned()),
                text.len(),
            )),
            ChatEvent::BlockComplete { context, text } => Some((
                "BlockComplete",
                Some(context.chat_id.as_str().to_owned()),
                Some(context.turn_id.as_str().to_owned()),
                text.len(),
            )),
            ChatEvent::UserMessagesAdopted { items, .. } => {
                Some(("UserMessagesAdopted", None, None, items.len()))
            }
            ChatEvent::Done { context } | ChatEvent::DoneWithDurationMs { context, .. } => Some((
                "Done",
                Some(context.chat_id.as_str().to_owned()),
                Some(context.turn_id.as_str().to_owned()),
                0,
            )),
            _ => None,
        };
        let send_result = self.tx.send(projected);
        if let Some((kind, chat_id, turn_id, size)) = diagnostic {
            match send_result {
                Ok(()) if kind == "Text" => log::trace!(
                    target: crate::LOG_TARGET,
                    "event_delivery boundary=runtime_to_sdk mode={} kind={} chat_id={} turn_id={} size={} outcome=sent",
                    mode,
                    kind,
                    chat_id.as_deref().unwrap_or("-"),
                    turn_id.as_deref().unwrap_or("-"),
                    size
                ),
                Ok(()) => log::debug!(
                    target: crate::LOG_TARGET,
                    "event_delivery boundary=runtime_to_sdk mode={} kind={} chat_id={} turn_id={} size={} outcome=sent",
                    mode,
                    kind,
                    chat_id.as_deref().unwrap_or("-"),
                    turn_id.as_deref().unwrap_or("-"),
                    size
                ),
                Err(_) => log::warn!(
                    target: crate::LOG_TARGET,
                    "event_delivery boundary=runtime_to_sdk mode={} kind={} chat_id={} turn_id={} size={} outcome=receiver_closed",
                    mode,
                    kind,
                    chat_id.as_deref().unwrap_or("-"),
                    turn_id.as_deref().unwrap_or("-"),
                    size
                ),
            }
        } else if send_result.is_err() {
            log::warn!(
                target: crate::LOG_TARGET,
                "event_delivery boundary=runtime_to_sdk mode={} kind=other outcome=receiver_closed",
                mode
            );
        }
    }
}

impl crate::application::main_loop::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::application::main_loop::RuntimeStreamEvent,
    ) -> crate::application::main_loop::EventFuture<'a> {
        Box::pin(async move {
            self.send_projected(self.project_and_mark(event), "async");
        })
    }

    fn try_send_event(&self, event: crate::application::main_loop::RuntimeStreamEvent) {
        self.send_projected(self.project_and_mark(event), "try");
    }

    fn send_domain_event<'a>(
        &'a self,
        event: crate::domain::agent_run::RunDomainEvent,
    ) -> crate::application::main_loop::EventFuture<'a> {
        Box::pin(async move {
            let projected = crate::adapters::event_projection::project_domain_event(event);
            if self.tx.send(projected).is_err() {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "event_delivery boundary=runtime_to_sdk mode=domain kind=run_domain_event outcome=receiver_closed"
                );
            }
        })
    }
}
