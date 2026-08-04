use super::*;
use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, EventFuture, RuntimeRunContext, RuntimeStreamEvent,
};
use crate::domain::agent_run::{RunDomainEvent, RunId};
use sdk::ids::{ChatId, ChatRunId};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct RecordingSink {
    terminal_events: Arc<Mutex<Vec<&'static str>>>,
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move {
            let terminal = match event {
                RuntimeStreamEvent::DoneWithDuration { .. } => Some("completed"),
                RuntimeStreamEvent::Cancelled { .. } => Some("cancelled"),
                _ => None,
            };
            if let Some(terminal) = terminal {
                self.terminal_events.lock().unwrap().push(terminal);
            }
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        let terminal = match event {
            RuntimeStreamEvent::DoneWithDuration { .. } => Some("completed"),
            RuntimeStreamEvent::Cancelled { .. } => Some("cancelled"),
            _ => None,
        };
        if let Some(terminal) = terminal {
            self.terminal_events.lock().unwrap().push(terminal);
        }
    }
}

#[tokio::test]
async fn completed_seal_after_cancelled_step_projects_only_cancelled_terminal() {
    let sink = RecordingSink::default();
    let turn_context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let task_access = crate::application::run::test_task_access();
    let mut observer = ChatStreamEventObserver {
        sink: ChatEventSinkHandle::new(sink.clone()),
        session_id: "session",
        turn_context: &turn_context,
        task_access: &task_access,
        model: "model",
        started_at: std::time::Instant::now(),
        step_count: 1,
        messages_snapshot: Vec::new(),
    };

    observer
        .emit(vec![RunDomainEvent::Completed {
            run_id: RunId::new_v7(),
            parent_run_id: None,
            result: String::new(),
            user_cancelled_step: true,
        }])
        .await
        .expect("project terminal event");

    assert_eq!(
        sink.terminal_events.lock().unwrap().as_slice(),
        &["cancelled"]
    );
}
