mod agent_calls;
mod ask_user;
mod compact;
mod compact_outcome;
pub(crate) mod config_reload;
mod engine;
mod events;
mod finalize;
mod hook_ui;
mod idle_commands;
mod idle_lifecycle;
mod input_gate;
#[cfg(test)]
mod input_gate_reset_withdraw_tests;
#[cfg(test)]
mod input_gate_tests;
mod input_log;
mod llm_log;
mod loop_context;
mod loop_helpers;
mod loop_phases;
mod loop_runner;
#[cfg(test)]
mod loop_runner_tests;
pub(crate) mod memory_inject;
mod non_agent;
mod permissions;
mod post_batch;
mod queue;
pub(crate) mod reflection;
mod snapshot_registry;
mod stall;
mod state;
mod stream_handler;
#[cfg(test)]
mod stream_handler_tests;
mod task_reminder;
mod task_snapshot;
mod tool_fuse;
mod tool_identity;
mod tools;

pub use events::{
    ChatEventSink, CompactStage, EventFuture, RuntimeHookEvent, RuntimeHookEventStatus,
    RuntimeHookExecutionResult, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
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
pub use state::{ChatLoopFsm, ChatLoopState, ChatLoopTransition};
pub use stream_handler::RuntimeStreamHandler;
