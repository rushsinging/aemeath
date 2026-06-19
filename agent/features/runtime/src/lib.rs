/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:runtime";

pub mod api;
pub mod business;
pub mod contract;
pub mod core;
pub mod gateway;
pub mod tool_adapter;
pub mod utils;
