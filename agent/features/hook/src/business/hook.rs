//! Hook 执行引擎
//!
//! 在 aemeath 生命周期关键点执行用户自定义 shell 命令。
//! 通过 stdin 传入 JSON 数据，通过 exit code 控制行为。
//!
//! ## 模块结构
//! - `data` — 事件数据模型（HookInput, HookData, 各事件数据结构）
//! - `result` — 执行结果与 JSON 输出模型（HookResult, HookJsonOutput）
//! - `runner` — 核心执行引擎（HookRunner 構造、匹配、执行）
//! - `events` — 事件便捷方法（为每个生命周期事件提供类型安全的 API）

mod data;
mod events;
mod result;
mod runner;

// 数据模型 —— 公开所有类型
pub use data::*;
// 结果模型
pub use result::*;
// 运行器
pub use runner::HookRunner;

#[cfg(test)]
#[path = "hook/tests.rs"]
mod tests;
