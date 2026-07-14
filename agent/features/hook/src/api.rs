// ── 新契约类型（#922 Published Language） ──
pub use crate::contract::{
    classify_directive, HookClass, HookDirective, HookExecution, HookExecutionStatus,
    HookInvocation, HookOutcome, HookPoint, HookPointMetadata, HookPort, HookReason,
};

// ── 新 typed input structs ──
pub use crate::contract::invocation::*;

// ── 旧类型（经 gateway re-export，#926 退役后移除） ──
pub use crate::gateway::*;

#[cfg(test)]
mod tests {
    #[test]
    fn test_api_reexport_resolves() {
        // 编译期即验证：上述 re-export 项均可解析。
        fn _assert<T>() {}
        _assert::<super::HookRunner>();
        _assert::<super::HookData>();
        _assert::<super::HookResult>();
        _assert::<super::HookJsonOutput>();
        _assert::<super::HookInvocation>();
        _assert::<super::HookOutcome>();
        _assert::<super::HookDirective>();
    }
}
