mod agent_calls;
mod ask_user;
pub(crate) mod config_reload;
mod events;
pub(crate) mod finalize;
mod hook_ui;
mod idle_commands;
mod idle_lifecycle;
mod input_gate;
#[cfg(test)]
mod input_gate_reset_withdraw_tests;
#[cfg(test)]
mod input_gate_tests;
mod input_log;
mod loop_context;
mod loop_phases;
mod loop_runner;
#[cfg(test)]
mod loop_runner_tests;
mod non_agent;
mod post_batch;
#[cfg(test)]
mod pre_compact_trigger_tests;
mod queue;
pub(crate) mod reflection;
#[cfg(test)]
mod reflection_trigger_tests;
pub(crate) mod run_input_buffer;
mod snapshot_registry;
pub(crate) mod stall;
mod stream_handler;
#[cfg(test)]
mod stream_handler_tests;
mod task_reminder;
mod task_snapshot;
mod tools;

pub use events::{
    ChatEventSink, ChatEventSinkHandle, CompactStage, EventFuture, RuntimeHookEvent,
    RuntimeHookEventStatus, RuntimeHookExecutionResult, RuntimeHookMessage, RuntimeHookMessageKind,
    RuntimeResumedSessionStep, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
pub use input_gate::{
    apply_gate, drain_sources, run_loop_gate, ControlCommand, ControlCommandKind,
    EmptyInputEventDrainPort, EmptyQueueDrainPort, GateDecision, GateKind, GateOutcome,
    InputEventDrainPort, InputEventFuture, InputEventOptFuture, PendingCommand, PendingInputBuffer,
};
pub use input_log::logged_input_messages;
pub use loop_context::{ChatLoopContext, SwitchClientFn};
pub use loop_runner::process_chat_loop;
pub use queue::{append_queued_input, QueueDrainPort, QueueFuture};
pub(crate) use stream_handler::{InvocationEventReducer, InvocationResponse};
