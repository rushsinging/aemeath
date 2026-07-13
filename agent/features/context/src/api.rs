//! Context crate 的 API facade——跨 crate 访问统一经 `context::api`。
//!
//! 重导出 contract + gateway。

pub use crate::contract::*;
pub use crate::gateway::*;
