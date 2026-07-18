#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:composition";

pub mod app;
pub mod memory;
pub mod provider;
pub mod runtime;
pub mod tools;
pub mod update;

/// Re-export 版本号，CLI 经 composition 间接引用 `share::version()`
/// 而不直接依赖 shared（守薄入口守卫）。
pub use share::{version, COMPILED_VERSION};
