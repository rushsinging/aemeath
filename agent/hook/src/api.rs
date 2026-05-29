//! Hook crate 对外公开门面。
//!
//! 仅暴露 runtime 实际消费的类型，内部模块（data/result/runner/events）保持私有。

pub use crate::hook::{
    CompactHookData, HookData, HookJsonOutput, HookResult, HookRunner, PermissionHookData,
    StopHookData, ToolHookData,
};

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
    }
}
