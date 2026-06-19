/// business/agent/mod.rs — Agent 模型 + Looping 引擎
#[allow(clippy::module_inception)]
pub mod agent;
pub mod runner;

pub use agent::{Agent, ToolCall, ToolExecution};
