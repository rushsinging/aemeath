pub use crate::contract::*;
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
    }
}
