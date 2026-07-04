use crate::business::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use share::message::Message;
use std::future::Future;
use std::pin::Pin;

pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + 'a>>;

pub trait QueueDrainPort: Clone + Send + Sync + 'static {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a>;
}

pub async fn append_queued_input<Q, S>(queue: &Q, sink: &S, messages: &mut Vec<Message>) -> bool
where
    Q: QueueDrainPort,
    S: ChatEventSink,
{
    let Some(queued) = queue.drain_queued_input().await else {
        return false;
    };
    if queued.is_empty() {
        return false;
    }

    for input in queued {
        messages.push(Message::user(input));
    }

    sink.send_event(RuntimeStreamEvent::PostToolExecutionSync {
        messages: messages.clone(),
    })
    .await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::chat::looping::events::{EventFuture, RuntimeStreamEvent};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestQueueDrainPort {
        queued: Arc<Mutex<Option<Vec<String>>>>,
    }

    impl TestQueueDrainPort {
        fn new(queued: Option<Vec<String>>) -> Self {
            Self {
                queued: Arc::new(Mutex::new(queued)),
            }
        }
    }

    impl QueueDrainPort for TestQueueDrainPort {
        fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
            Box::pin(async move { self.queued.lock().unwrap().take() })
        }
    }

    #[derive(Clone, Default)]
    struct TestEventSink {
        events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
    }

    impl ChatEventSink for TestEventSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                self.events.lock().unwrap().push(event);
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[tokio::test]
    async fn append_queued_input_appends_and_syncs() {
        let queue = TestQueueDrainPort::new(Some(vec![
            "queued one".to_string(),
            "queued two".to_string(),
        ]));
        let sink = TestEventSink::default();
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue, &sink, &mut messages).await;

        assert!(appended);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].text_content(), "queued one");
        assert_eq!(messages[2].text_content(), "queued two");

        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            RuntimeStreamEvent::PostToolExecutionSync {
                messages: sync_messages,
            } => {
                assert_eq!(sync_messages.len(), 3);
                assert_eq!(sync_messages[2].text_content(), "queued two");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn append_queued_input_empty_vec_returns_false() {
        let queue = TestQueueDrainPort::new(Some(Vec::new()));
        let sink = TestEventSink::default();
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue, &sink, &mut messages).await;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn append_queued_input_none_returns_false() {
        let queue = TestQueueDrainPort::new(None);
        let sink = TestEventSink::default();
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue, &sink, &mut messages).await;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
        assert!(sink.events.lock().unwrap().is_empty());
    }
}
