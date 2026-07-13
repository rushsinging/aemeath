//! Hook Published Language 契约。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md`。
//! 定义 typed dispatch、能力矩阵、执行协议真值表和 HookPort。
//!
//! 本模块只冻结契约类型，不实现 dispatcher/process/retry（#923/#924）。

pub mod invocation;
pub mod metadata;
pub mod outcome;
pub mod port;
pub mod protocol;

pub use invocation::*;
pub use metadata::*;
pub use outcome::*;
pub use port::*;
pub use protocol::classify_directive;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
