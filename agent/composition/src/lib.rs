#![deny(clippy::print_stdout, clippy::print_stderr)]

mod constants;
mod state;
pub(crate) use constants::LOG_TARGET;

pub mod app;
pub mod audit;
pub mod delivery_logging;
pub mod memory;
/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub mod provider;
pub mod runtime;
pub mod systemone;
pub mod tools;
pub mod update;

/// Re-export 版本号，CLI 经 composition 间接引用 `share::version()`
/// 而不直接依赖 shared（守薄入口守卫）。
pub use share::{version, COMPILED_VERSION};

/// Re-export git 子进程统一窄面：CLI（TUI 工作区元数据）经 composition 间接
/// 调用 project 的 `run_git_command`，不直接依赖 features crate（守薄入口守卫）。
pub use project::{run_git_command, GitCommandOutcome, GitOperationError};
