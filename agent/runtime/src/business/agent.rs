/// business/agent/mod.rs — Agent 模型 + Looping 引擎
#[allow(clippy::module_inception)]
pub mod agent;
#[cfg(test)]
mod agent_tests;
pub mod runner;

pub use agent::{Agent, ToolCall, ToolResultTuple};
