//! Hook：进程级 hook 的订阅、派发与执行观察。
//!
//! # Published Language（四类语法，#1708 收敛）
//!
//! | 类 | 实体 |
//! |---|---|
//! | 工厂 | `wire_hook_dispatcher`（返回 `Arc<dyn HookDispatcher>`；实现体 Dispatcher 与 SubscriptionError 均 crate 私有） |
//! | Role | `HookDispatcher`（原 HookPort，派发 port）、`HookExecutionObserver`（执行观察注入）、`HookCancellationSignal`（取消信号注入） |
//! | Data | `HookInvocationData`（26 变体字段内联，23 个 Input 载荷 struct 消亡导出）、`HookOutcomeData`、`HookPointData`、`HookDirectiveData`、`HookClassData`、`HookMatcherData`、`HookReasonData`、`HookExecutionData`、`HookExecutionStatusData`、`HookDisplayMessageData`、`HookDisplayMessageKindData`、`HookDispatchContextData`、`HookExecutionEventData`、`HookExecutionTerminalData` |
//! | Error | `share::error::DomainError`（订阅配置非法经 wire 折叠） |
//!
//! 按 docs/design/03-engineering/05-published-language.md 执行 SOP 收敛。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:hook";

mod adapters;
mod domain;
mod ports;

// 稳定 façade：仅导出生产 Dispatcher 与领域 PL。
// Executor / RawExecution / ExecutionFault / ProcessDriverExecutor 等技术类型
// 是 adapters detail，**NEVER** 进入 crate 公开面。
pub use adapters::config::wire_hook_dispatcher;
pub use domain::{
    HookClassData, HookDirectiveData, HookDisplayMessageData, HookDisplayMessageKindData,
    HookExecutionData, HookExecutionStatusData, HookInvocationData, HookMatcherData,
    HookOutcomeData, HookPointData, HookReasonData,
};
pub use ports::{
    HookCancellationSignal, HookDispatchContextData, HookDispatcher, HookExecutionEventData,
    HookExecutionObserver, HookExecutionTerminalData,
};
