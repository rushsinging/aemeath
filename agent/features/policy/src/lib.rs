/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:policy";

pub mod api;
mod business;
pub mod contract;
pub mod gateway;
