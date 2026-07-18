/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:hook";

mod adapters;
mod domain;
mod ports;

pub use domain::invocation::*;
pub use domain::{
    classify_directive, HookClass, HookDirective, HookExecution, HookExecutionStatus,
    HookInvocation, HookOutcome, HookPoint, HookPointMetadata, HookReason,
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
        classify_directive, HookClass, HookDirective, HookExecution, HookExecutionStatus,
        HookInvocation, HookOutcome, HookPoint, HookPointMetadata, HookReason,
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
