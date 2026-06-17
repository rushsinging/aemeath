#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:tools";

pub mod api;
pub mod contract;
pub mod gateway;

mod business;
mod core;
