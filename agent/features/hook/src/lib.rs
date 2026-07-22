/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:hook";

mod adapters;
mod domain;
mod ports;

// 稳定 façade：仅导出生产 Dispatcher（含设计常量 MAX_ATTEMPTS）+ 领域 PL。
// Executor / RawExecution / ExecutionFault / ProcessDriverExecutor 等技术类型
// 是 adapters detail，**NEVER** 进入 crate 公开面。
pub use adapters::config::build_dispatcher;
pub use adapters::dispatcher::{Dispatcher, MAX_ATTEMPTS};
pub use domain::invocation::*;
pub use domain::{
    classify_directive, ClassifyError, HookClass, HookCommand, HookDirective, HookDisplayMessage,
    HookDisplayMessageKind, HookExecution, HookExecutionStatus, HookFailurePolicy, HookInvocation,
    HookMatcher, HookOutcome, HookPoint, HookPointMetadata, HookReason, HookSubscription,
    ProtocolViolation, SubscriptionError,
};
pub use ports::{HookDispatchContext, HookPort};
