use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainEvent {
    pub aggregate_id: String,
    pub event_type: DomainEventType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainEventType {
    ChatMessageAdded,
}

#[derive(Clone, Debug)]
pub struct EventBus {
    events: broadcast::Sender<DomainEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (events, _) = broadcast::channel(capacity);
        Self { events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.events.subscribe()
    }

    pub fn publish(&self, event: DomainEvent) -> usize {
        self.events.send(event).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_publishes_domain_event_to_subscriber() {
        let bus = EventBus::new(16);
        let mut subscriber = bus.subscribe();
        let event = DomainEvent {
            aggregate_id: "chat-1".to_string(),
            event_type: DomainEventType::ChatMessageAdded,
        };

        let receiver_count = bus.publish(event.clone());

        assert_eq!(receiver_count, 1);
        assert_eq!(subscriber.try_recv(), Ok(event));
    }

    #[test]
    fn test_event_bus_reports_zero_when_no_subscribers() {
        let bus = EventBus::new(16);
        let event = DomainEvent {
            aggregate_id: "chat-1".to_string(),
            event_type: DomainEventType::ChatMessageAdded,
        };

        let receiver_count = bus.publish(event);

        assert_eq!(receiver_count, 0);
    }

    #[test]
    fn test_event_bus_subscriber_only_receives_new_events() {
        let bus = EventBus::new(16);
        bus.publish(DomainEvent {
            aggregate_id: "chat-1".to_string(),
            event_type: DomainEventType::ChatMessageAdded,
        });
        let mut subscriber = bus.subscribe();

        let event = DomainEvent {
            aggregate_id: "chat-2".to_string(),
            event_type: DomainEventType::ChatMessageAdded,
        };
        bus.publish(event.clone());

        assert_eq!(subscriber.try_recv(), Ok(event));
    }
}
