#![deny(clippy::print_stdout, clippy::print_stderr)]

//! agent 下所有库的共享依赖层。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:shared";

pub mod adapter;
pub mod config;
pub mod error;
pub mod memory;
pub mod memory_ops;
pub mod message;
pub mod session_types;
pub mod skill_ops;
pub mod string_idx;
pub mod task;
pub mod task_ops;
pub mod tool;
