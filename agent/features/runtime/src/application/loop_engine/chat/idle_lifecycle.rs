//! Session idle lifecycle. Active Run state belongs exclusively to agent_run.

use crate::application::loop_engine::chat::apply_gate;
use crate::application::loop_engine::chat::events::{ChatEventSink, RuntimeStreamEvent};
use crate::application::loop_engine::chat::input_gate::{
    event_kind_name, GateKind, InputEventDrainPort, PendingCommand, PendingInputBuffer,
};
use share::message::Message;
use share::reasoning::ReasoningLevel;

fn requested_level_for_thinking(
    reasoning: &std::sync::Mutex<ReasoningLevel>,
    desired: Option<bool>,
) -> ReasoningLevel {
    let mut current = reasoning.lock().unwrap_or_else(|error| error.into_inner());
    let enabled = desired.unwrap_or(matches!(*current, ReasoningLevel::Off));
    *current = if enabled {
        ReasoningLevel::Medium
    } else {
        ReasoningLevel::Off
    };
    *current
}

pub(crate) async fn execute_set_thinking<S>(
    reasoning: &std::sync::Mutex<ReasoningLevel>,
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
    Resumed {
        segment_id: String,
        adopted_messages: Vec<(sdk::InputId, Message)>,
        adopted_events: Vec<sdk::ChatInputEvent>,
    },
    ResetRequested,
    Shutdown,
    CommandRequested(PendingCommand),
}

async fn await_idle_input<I: InputEventDrainPort>(
    input_events: &I,
    pending: &mut PendingInputBuffer,
) -> IdleResult {
    let event = match pending.pop_front() {
        Some(event) => Some(event),
        None => input_events.recv_next_input().await,
    };
    match event {
        Some(event) => {
            log::debug!(
                target: crate::LOG_TARGET,
                "session idle woken by event kind={}",
                event_kind_name(&event)
            );
            pending.push(event);
            IdleResult::Resumed {
                segment_id: String::new(),
                adopted_messages: Vec::new(),
                adopted_events: Vec::new(),
            }
        }
        None => IdleResult::Shutdown,
    }
}

pub(crate) async fn idle_until_resume_or_shutdown<I, S>(
    input_events: &I,
    sink: &S,
    pending: &mut PendingInputBuffer,
    task_access: &dyn task::TaskAccess,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    loop {
        match await_idle_input(input_events, pending).await {
            IdleResult::Resumed { .. } => {
                let segment_id = sdk::ChatId::new_v7().to_string();
                let gate = apply_gate(GateKind::BeforeLlm, pending, sink, task_access, true).await;
                if let Some(command) = gate.pending_command {
                    return IdleResult::CommandRequested(command);
                }
                if gate.reset_requested {
                    return IdleResult::ResetRequested;
                }
                if gate.appended_user_messages > 0 {
                    return IdleResult::Resumed {
                        segment_id,
                        adopted_messages: gate.adopted_messages,
                        adopted_events: gate.adopted_events,
                    };
                }
            }
            IdleResult::ResetRequested => return IdleResult::ResetRequested,
            IdleResult::Shutdown => return IdleResult::Shutdown,
            IdleResult::CommandRequested(command) => return IdleResult::CommandRequested(command),
        }
    }
}
