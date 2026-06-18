#![deny(clippy::print_stdout, clippy::print_stderr)]

//! agent 下所有库的共享依赖层。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:shared";

/// 应用版本号，来源于 build.rs 从 git tag 注入的 `AEMEATH_VERSION`；
/// 取不到时 fallback 到 `Cargo.toml` 的 `version`（占位符 `0.0.0`）。
/// 全仓库所有需要版本号的地方 MUST 引用此常量，NEVER 直接用 `CARGO_PKG_VERSION`。
pub const VERSION: &str = match option_env!("AEMEATH_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

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
