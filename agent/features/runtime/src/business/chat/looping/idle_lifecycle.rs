//! Session idle lifecycle. Active Run state belongs exclusively to agent_run.

use crate::business::chat::looping::apply_gate;
use crate::business::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use crate::business::chat::looping::input_gate::{
    event_kind_name, GateKind, InputEventDrainPort, PendingCommand, PendingInputBuffer,
};
use crate::business::session::ChatChain;
use crate::LOG_TARGET;

pub(crate) async fn execute_set_thinking<S>(
    client: &provider::api::LlmClient,
    sink: &S,
    desired: Option<bool>,
) where
    S: ChatEventSink,
{
    use provider::api::ReasoningLevel;
    let current = client.current_reasoning_level();
    let new_state = desired.unwrap_or(matches!(current, ReasoningLevel::Off));
    let level = if new_state {
        ReasoningLevel::Medium
    } else {
        ReasoningLevel::Off
    };
    client.set_reasoning_level(level);
    let label = if new_state { "ON" } else { "OFF" };
    sink.send_event(RuntimeStreamEvent::ThinkingChanged { enabled: new_state })
        .await;
    sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
        "[thinking mode: {label}]"
    )))
    .await;
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
    task_store: &storage::api::TaskStore,
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
