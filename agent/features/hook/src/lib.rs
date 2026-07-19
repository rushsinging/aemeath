/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:hook";

mod adapters;
mod domain;
mod ports;

// 稳定 façade：仅导出生产 Dispatcher（含设计常量 MAX_ATTEMPTS）+ 领域 PL。
// Executor / RawExecution / ExecutionFault / ProcessDriverExecutor 等技术类型
// 是 adapters detail，**NEVER** 进入 crate 公开面。
pub use adapters::dispatcher::{Dispatcher, MAX_ATTEMPTS};
pub use domain::invocation::*;
pub use domain::{
    classify_directive, ClassifyError, HookClass, HookCommand, HookDirective, HookDisplayMessage,
    HookDisplayMessageKind, HookExecution, HookExecutionStatus, HookFailurePolicy, HookInvocation,
    HookMatcher, HookOutcome, HookPoint, HookPointMetadata, HookReason, HookSubscription,
    ProtocolViolation, SubscriptionError,
};
pub use ports::HookPort;

/// 迁移期兼容 façade；Runtime 消费切换与旧 HookRunner 退役由 #925/#926 承接。
pub mod api {
    pub use crate::adapters::legacy::{
        is_blocking, CompactHookData, HookData, HookInput, HookJsonOutput, HookResult, HookRunner,
        PermissionHookData, StopHookData, ToolHookData,
    };
    pub use crate::domain::invocation::*;
    pub use crate::domain::{
        classify_directive, ClassifyError, HookClass, HookDirective, HookDisplayMessage,
        HookDisplayMessageKind, HookExecution, HookExecutionStatus, HookInvocation, HookOutcome,
        HookPoint, HookPointMetadata, HookReason, ProtocolViolation,
    };
    pub use crate::ports::HookPort;

    #[cfg(test)]
    mod tests {
        #[test]
        fn test_api_reexport_resolves() {
            fn assert_type<T>() {}
            assert_type::<super::HookRunner>();
            assert_type::<super::HookData>();
            assert_type::<super::HookResult>();
            assert_type::<super::HookJsonOutput>();
            assert_type::<super::HookInvocation>();
            assert_type::<super::HookOutcome>();
            assert_type::<super::HookDirective>();
        }
    }
}
