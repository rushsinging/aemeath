/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量.
pub const LOG_TARGET: &str = "aemeath:agent:runtime";

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

pub use application::client::{from_args, from_args_with_workspace, AgentClientImpl};
pub use sdk::{
    AgentClient, ChangeSet, ChatEvent, ChatRequest, ChatStream, CostInfo, ProjectContext,
    SessionSnapshot, TaskSummary,
};
