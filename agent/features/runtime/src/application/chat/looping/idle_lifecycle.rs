//! Session idle lifecycle. Active Run state belongs exclusively to agent_run.

use crate::application::chat::looping::apply_gate;
use crate::application::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use crate::application::chat::looping::input_gate::{
    event_kind_name, GateKind, InputEventDrainPort, PendingCommand, PendingInputBuffer,
};
use crate::LOG_TARGET;
use context::session::ChatChain;
use share::reasoning::ReasoningLevel;
use workflow::api::ReasoningPort;

fn requested_level_for_thinking(
    reasoning: &dyn ReasoningPort,
    desired: Option<bool>,
) -> ReasoningLevel {
    let current = reasoning.current_requested_level();
    let enabled = desired.unwrap_or(matches!(current, ReasoningLevel::Off));
    reasoning.set_level(if enabled {
        ReasoningLevel::Medium
    } else {
        ReasoningLevel::Off
    })
}

pub(crate) async fn execute_set_thinking<S>(
    reasoning: &dyn ReasoningPort,
    sink: &S,
    desired: Option<bool>,
) -> ReasoningLevel
where
    S: ChatEventSink,
{
    let level = requested_level_for_thinking(reasoning, desired);
    let enabled = !matches!(level, ReasoningLevel::Off);
    let label = if enabled { "ON" } else { "OFF" };
    sink.send_event(RuntimeStreamEvent::ThinkingChanged { enabled })
        .await;
    sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
        "[thinking mode: {label}]"
    )))
    .await;
    level
}

pub(crate) enum IdleResult {
    Resumed(String),
    Shutdown,
    CommandRequested(PendingCommand),
}

async fn await_idle_input<I: InputEventDrainPort>(
    input_events: &I,
    pending: &mut PendingInputBuffer,
) -> IdleResult {
    match input_events.recv_next_input().await {
        Some(event) => {
            log::debug!(
                target: LOG_TARGET,
                "session idle woken by event kind={}",
                event_kind_name(&event)
            );
            pending.push(event);
            IdleResult::Resumed(String::new())
        }
        None => IdleResult::Shutdown,
    }
}

pub(crate) async fn idle_until_resume_or_shutdown<I, S>(
    input_events: &I,
    sink: &S,
    pending: &mut PendingInputBuffer,
    chain: &mut ChatChain,
    task_store: &storage::TaskStore,
    task_access: &dyn task::TaskAccess,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    loop {
        match await_idle_input(input_events, pending).await {
            IdleResult::Resumed(_) => {
                let segment_id = sdk::ChatId::new_v7().to_string();
                let gate = apply_gate(
                    GateKind::BeforeLlm,
                    pending,
                    sink,
                    chain,
                    &segment_id,
                    task_store,
                    task_access,
                    true,
                )
                .await;
                if let Some(command) = gate.pending_command {
                    return IdleResult::CommandRequested(command);
                }
                if gate.appended_user_messages > 0 {
                    return IdleResult::Resumed(segment_id);
                }
            }
            IdleResult::Shutdown => return IdleResult::Shutdown,
            IdleResult::CommandRequested(command) => return IdleResult::CommandRequested(command),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use workflow::api::{ReasoningNode, ReasoningObservation, ReasoningSignal};

    struct StubReasoningPort {
        requested: Mutex<ReasoningLevel>,
    }

    impl ReasoningPort for StubReasoningPort {
        fn observe(&self, _signal: ReasoningSignal) -> ReasoningObservation {
            ReasoningObservation {
                previous: ReasoningNode::Idle,
                current: ReasoningNode::Idle,
                requested: self.current_requested_level(),
            }
        }

        fn current_requested_level(&self) -> ReasoningLevel {
            *self.requested.lock().unwrap()
        }

        fn set_level(&self, level: ReasoningLevel) -> ReasoningLevel {
            *self.requested.lock().unwrap() = level;
            level
        }

        fn reset_default_level(&self, level: ReasoningLevel) -> ReasoningLevel {
            *self.requested.lock().unwrap() = level;
            level
        }
    }

    #[test]
    fn requested_level_for_thinking_uses_port_state_and_writes_toggle() {
        let port = StubReasoningPort {
            requested: Mutex::new(ReasoningLevel::Off),
        };

        assert_eq!(
            requested_level_for_thinking(&port, Some(true)),
            ReasoningLevel::Medium
        );
        assert_eq!(port.current_requested_level(), ReasoningLevel::Medium);
        assert_eq!(
            requested_level_for_thinking(&port, None),
            ReasoningLevel::Off
        );
    }
}
