//! Policy：权限决策的单一入口。
//!
//! # Published Language（#1710 收敛后，8 实体）
//!
//! | 类别 | 实体 | 消费者 |
//! |---|---|---|
//! | Port | `PolicyPort` | composition（装配）、runtime（决策调用） |
//! | 决策契约 | `PolicyDecision`、`PolicyRequest`、`ApprovalSubject`、`PolicyReason`（Port 签名载荷，实现方必然公开） | runtime（含 fake 实现） |
//! | 配置 | `AllowAllPolicy`、`ConfiguredPolicy`、`PolicyMode`、`PolicyModeSource` | composition |
//!
//! 边界约定：`StandardPolicy` 为 crate 私有
//! （契约测试在 crate 内）；`AuthorizationContext` 归属 tools crate，本 crate
//! 不做转发（消费方直接 `tools::AuthorizationContext`）。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:policy";
/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
pub(crate) mod contract;
mod domain;

pub use adapters::{AllowAllPolicy, ConfiguredPolicy};
pub use domain::{
    ApprovalSubject, PolicyDecision, PolicyMode, PolicyModeSource, PolicyPort, PolicyReason,
    PolicyRequest,
};
