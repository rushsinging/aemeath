//! Policy：权限决策的单一入口。
//!
//! # Published Language（四类语法）
//!
//! | 类 | 实体 | 消费者 |
//! |---|---|---|
//! | Role | `Policy`（evaluate + current_mode，同源读行为）、`PolicyModeReader`（mode 注入，composition 实现） | runtime、composition |
//! | Data | `PolicyRequestData`、`PolicyDecisionData`、`PolicyModeData`、`ApprovalSubjectData`、`PolicyReasonData`（载荷） | 随签名 |
//!
//! `ConfiguredPolicy`（生产装配入口）与 `AllowAllPolicy`（runtime 测试策略值）
//! 保留导出——语法特例：策略实现体；Standard 收窄 crate 内。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:policy";
/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
pub(crate) mod contract;
mod domain;

pub use adapters::{AllowAllPolicy, ConfiguredPolicy};
pub use domain::{
    ApprovalSubjectData, Policy, PolicyDecisionData, PolicyModeData, PolicyModeReader,
    PolicyReasonData, PolicyRequestData,
};
