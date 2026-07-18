//! Hook Published Language 与纯领域策略。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md`。
//! 定义 typed invocation、能力矩阵、Outcome 与执行协议真值表。
//!
//! 本模块不实现 dispatcher/process/retry（#923/#924）。

pub mod invocation;
pub mod metadata;
pub mod outcome;
pub mod protocol;

pub use invocation::*;
pub use metadata::*;
pub use outcome::*;
pub use protocol::classify_directive;

#[cfg(test)]
#[path = "domain/tests.rs"]
mod tests;
