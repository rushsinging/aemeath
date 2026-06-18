#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:composition";

pub mod app;
pub mod provider;
pub mod runtime;
pub mod tools;
