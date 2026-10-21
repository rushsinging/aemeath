//! Harness 边界错误：复用全仓唯一跨界错误类型（形态 B）。

use share::error::DomainError;

/// 本 crate 边界错误别名（domain 固定为 `harness`）。
pub type HarnessError = DomainError;
