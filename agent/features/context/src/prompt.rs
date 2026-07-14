//! Prompt & Guidance 能力。
//!
//! 原 `agent/features/prompt/` crate 整体并入 Context Management。

pub mod guidance;
mod security;
pub mod skill;

/// Prompt 能力日志 target。
pub const LOG_TARGET: &str = "aemeath:agent:prompt";
