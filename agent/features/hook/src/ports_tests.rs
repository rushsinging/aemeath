//! `HookDispatchContextData` 单元测试：cwd 与 session_id 的字段完整性。

#![cfg(test)]

use crate::ports::HookDispatchContextData;

/// dispatch context 默认不携带 session_id；builder 写入后访问器返回同一值。
#[test]
fn dispatch_context_session_id_builder_roundtrip() {
    let context = HookDispatchContextData::new("/tmp/aemeath-hook-workspace");
    assert_eq!(context.session_id(), None);

    let context = context.with_session_id("sess-ports-789");
    assert_eq!(
        context.session_id(),
        Some("sess-ports-789".to_string()).as_deref()
    );
    assert_eq!(
        context.cwd(),
        std::path::Path::new("/tmp/aemeath-hook-workspace")
    );
}
