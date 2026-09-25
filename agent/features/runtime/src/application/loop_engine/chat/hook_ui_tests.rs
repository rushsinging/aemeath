//! `dispatch_hook` 会话标识透传契约：context 必须携带调用方传入的 session_id。

#![cfg(test)]

use std::sync::Arc;

use hook::{HookDirective, HookDispatchContext, HookInvocation, HookOutcome, HookPort};

use super::hook_ui::dispatch_hook;

/// 记录 (触发点, context.session_id) 的 dispatch 轨迹。
type ContextDispatchLog = Arc<std::sync::Mutex<Vec<(hook::HookPoint, Option<String>)>>>;

#[derive(Clone, Default)]
struct ContextRecordingHookPort {
    dispatches: ContextDispatchLog,
}

#[async_trait::async_trait]
impl HookPort for ContextRecordingHookPort {
    async fn dispatch(
        &self,
        _invocation: HookInvocation,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> HookOutcome {
        unreachable!("主循环 hook 必须经 dispatch_at 携带 workspace 上下文");
    }

    async fn dispatch_at(
        &self,
        invocation: HookInvocation,
        context: HookDispatchContext,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> HookOutcome {
        self.dispatches
            .lock()
            .unwrap()
            .push((invocation.point(), context.session_id().map(str::to_string)));
        HookOutcome {
            executions: Vec::new(),
            directive: HookDirective::Continue,
            messages: Vec::new(),
            block_detail: None,
        }
    }
}

fn coordinator() -> crate::application::activity::ActivityCoordinator {
    crate::application::activity::ActivityCoordinator::new(
        crate::domain::agent_run::RunId::new("run-hook-ui"),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    )
}

/// dispatch_hook 把调用方传入的 session_id 写入 dispatch context：
/// PreToolUse（主循环代表路径）的 hook 子进程环境据此获得 AEMEATH_SESSION_ID。
#[tokio::test]
async fn dispatch_hook_passes_session_id_into_context() {
    let port = ContextRecordingHookPort::default();
    let activities = coordinator();

    dispatch_hook(
        &(Arc::new(port.clone()) as Arc<dyn HookPort>),
        &activities,
        &sdk::RunStepId::new("step-hook-ui"),
        HookInvocation::PreToolUse(hook::PreToolUseInput {
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
        }),
        std::path::Path::new("/tmp/aemeath-hook-ui-workspace"),
        "sess-hook-ui-1",
        &tokio_util::sync::CancellationToken::new(),
    )
    .await;

    let dispatches = port.dispatches.lock().unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].0, hook::HookPoint::PreToolUse);
    assert_eq!(
        dispatches[0].1.as_deref(),
        Some("sess-hook-ui-1"),
        "dispatch context 必须携带调用方 session_id"
    );
}
