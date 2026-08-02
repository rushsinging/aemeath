mod agent_calls;
pub(crate) mod config_reload;
pub(crate) mod events;
#[cfg(test)]
mod events_tests;
pub(crate) mod finalize;
mod hook_ui;
#[cfg(test)]
mod hook_ui_tests;
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
pub(crate) mod reflection;
#[cfg(test)]
mod reflection_trigger_tests;
pub(crate) mod run_input_buffer;
mod snapshot_registry;
pub(crate) mod stall;
mod stream_handler;
#[cfg(test)]
mod stream_handler_tests;
pub(crate) mod streaming_tool;
pub(crate) mod task_snapshot;
pub(crate) mod tools;

pub use events::{
    ChatEventSink, ChatEventSinkHandle, EventFuture, RuntimeHookEvent, RuntimeHookEventStatus,
    RuntimeHookExecutionResult, RuntimeHookMessage, RuntimeHookMessageKind,
    RuntimeResumedSessionStep, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
pub use input_gate::{
    apply_gate, GateKind, InputEventDrainPort, InputEventFuture, InputEventOptFuture,
    PendingCommand, PendingInputBuffer,
};
pub use input_log::logged_input_messages;
pub use loop_context::ChatLoopContext;
pub use loop_runner::process_chat_loop;
pub(crate) use stream_handler::{
    should_emit_model_stream_waiting, InvocationEventReducer, InvocationResponse,
};
