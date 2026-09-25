//! Policy：权限决策的单一入口。
//!
//! # Published Language（四类语法）
//!
//! | 类 | 实体 | 消费者 |
//! |---|---|---|
//! | Role | `Policy`（evaluate + current_mode，同源读行为） | runtime、composition |
//! | Data | `PolicyRequestData`、`PolicyDecisionData`、`PolicyModeData`、`ApprovalSubjectData`、`PolicyReasonData`（载荷） | 随签名 |
//!
//! 实现体全部 crate 私有：`configured(closure)`（mode 闭包动态供给）与
//! `allow_all()` 工厂返回 `Arc<dyn Policy>`——策略实现不单独占导出类，
//! mode 注入经 `Fn() -> PolicyModeData` 闭包（无需 trait）。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:policy";
/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
pub(crate) mod contract;
mod domain;

pub use adapters::{allow_all, configured};
pub use domain::{
    ApprovalSubjectData, Policy, PolicyDecisionData, PolicyModeData, PolicyReasonData,
    PolicyRequestData,
};
