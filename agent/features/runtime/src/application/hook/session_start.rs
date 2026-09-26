//! Session 级 SessionStart hook emit——会话身份确定时通知外部集成。
//!
//! 生命周期点（非闸门）：dispatch 失败只记日志，**NEVER** 阻断启动或恢复。
//! 调用方：startup（`--resume` 与新会话两路）、运行期 `/resume`。

use std::path::Path;
use std::sync::Arc;

use hook::{HookDispatchContextData, HookDispatcher, HookInvocationData};

/// Emit 一次 SessionStart：invocation payload 与 dispatch context 均携带
/// 当前 Main Session id，外部集成（如终端会话恢复）据此捕获/刷新会话。
///
/// 结果被丢弃——SessionStart 属生命周期点，hook 的 Block/失败对调用方无效；
/// 失败明细由 Dispatcher 保留在 HookOutcomeData.executions 并经日志 target 观测。
pub(crate) async fn emit_session_start(
    hook_port: &Arc<dyn HookDispatcher>,
    workspace_root: &Path,
    session_id: &str,
) {
    let invocation = HookInvocationData::SessionStart {
        session_id: session_id.to_string(),
    };
    let context = HookDispatchContextData::new(workspace_root).with_session_id(session_id);
    let outcome = hook_port
        .dispatch_at(
            invocation,
            context,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
    if outcome.directive != hook::HookDirectiveData::Continue {
        log::debug!(
            target: crate::LOG_TARGET,
            "session_start hook directive ignored (lifecycle point): {:?}",
            outcome.directive
        );
    }
}

#[cfg(test)]
#[path = "session_start_tests.rs"]
mod tests;
