//! SessionStart emit 单元测试：invocation payload 与 dispatch context 的会话标识完整性。

#![cfg(test)]

use std::sync::{Arc, Mutex};

use hook::{
    CancellationSignal, HookDirective, HookDispatchContext, HookInvocation, HookOutcome, HookPort,
};

use super::emit_session_start;

#[derive(Clone, Default)]
struct RecordingHookPort {
    invocations: Arc<Mutex<Vec<HookInvocation>>>,
    context_session_ids: Arc<Mutex<Vec<Option<String>>>>,
    context_cwds: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl HookPort for RecordingHookPort {
    async fn dispatch(
        &self,
        _invocation: HookInvocation,
        _cancellation: &dyn CancellationSignal,
    ) -> HookOutcome {
        unreachable!("SessionStart emit 必须经 dispatch_at 携带 workspace 上下文");
    }

    async fn dispatch_at(
        &self,
        invocation: HookInvocation,
        context: HookDispatchContext,
        _cancellation: &dyn CancellationSignal,
    ) -> HookOutcome {
        self.invocations.lock().unwrap().push(invocation);
        self.context_session_ids
            .lock()
            .unwrap()
            .push(context.session_id().map(str::to_string));
        self.context_cwds
            .lock()
            .unwrap()
            .push(context.cwd().display().to_string());
        HookOutcome {
            executions: Vec::new(),
            directive: HookDirective::Continue,
            messages: Vec::new(),
            block_detail: None,
        }
    }
}

/// emit 恰好产生一次 SessionStart：payload 与 context 均携带会话 id，cwd 取 workspace root。
#[tokio::test]
async fn emit_session_start_dispatches_once_with_session_identity() {
    let port = RecordingHookPort::default();
    emit_session_start(
        &(Arc::new(port.clone()) as Arc<dyn HookPort>),
        std::path::Path::new("/tmp/aemeath-emit-workspace"),
        "sess-emit-1",
    )
    .await;

    let invocations = port.invocations.lock().unwrap();
    assert_eq!(invocations.len(), 1);
    match &invocations[0] {
        HookInvocation::SessionStart(input) => {
            assert_eq!(input.session_id, "sess-emit-1");
        }
        other => panic!("必须是 SessionStart，实际 {other:?}"),
    }
    assert_eq!(
        port.context_session_ids.lock().unwrap()[0].as_deref(),
        Some("sess-emit-1")
    );
    assert_eq!(
        port.context_cwds.lock().unwrap()[0],
        "/tmp/aemeath-emit-workspace"
    );
}

/// hook 返回 Block/失败时 emit 不 panic、不上抛——生命周期点 NEVER 阻断调用方。
#[tokio::test]
async fn emit_session_start_never_blocks_caller_on_hook_failure() {
    struct FailingHookPort;

    #[async_trait::async_trait]
    impl HookPort for FailingHookPort {
        async fn dispatch(
            &self,
            _invocation: HookInvocation,
            _cancellation: &dyn CancellationSignal,
        ) -> HookOutcome {
            unreachable!();
        }

        async fn dispatch_at(
            &self,
            _invocation: HookInvocation,
            _context: HookDispatchContext,
            _cancellation: &dyn CancellationSignal,
        ) -> HookOutcome {
            HookOutcome {
                executions: Vec::new(),
                directive: HookDirective::Block {
                    reason: hook::HookReason::JsonBlock {
                        reason: "hook says no".to_string(),
                    },
                },
                messages: Vec::new(),
                block_detail: None,
            }
        }
    }

    // 仅断言正常返回：SessionStart 的 Block 语义对生命周期点无效，调用方继续。
    emit_session_start(
        &(Arc::new(FailingHookPort) as Arc<dyn HookPort>),
        std::path::Path::new("/tmp/aemeath-emit-workspace"),
        "sess-emit-2",
    )
    .await;
}
