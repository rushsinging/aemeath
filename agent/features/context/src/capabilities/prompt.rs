//! Prompt & Guidance 子模块（PromptPort）。
//!
//! 原 `agent/features/prompt/` crate 整体并入。

mod business;
pub(crate) mod gateway;

/// 旧 crate 的日志 target，prompt 内部 log 调用仍使用。
pub const LOG_TARGET: &str = "aemeath:agent:prompt";
