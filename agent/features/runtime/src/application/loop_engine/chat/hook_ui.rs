//! Hook dispatch helper — typed Runtime value mapping.

use crate::application::hook::outcome_mapper::{
    map_hook_outcome, RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookDisplayMessageKind,
};
use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeHookEvent, RuntimeHookEventStatus, RuntimeHookMessage,
    RuntimeHookMessageKind, RuntimeStreamEvent,
};
use hook::{HookDispatchContext, HookInvocation, HookPort};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// 执行一次 hook dispatch 并投影为 Runtime 可消费的纯值。
///
/// 当前工作区根必须按 invocation 显式传入，确保 worktree 切换不会复用旧 cwd。
pub(crate) async fn dispatch_hook<S: ChatEventSink>(
    hook_port: &Arc<dyn HookPort>,
    sink: &S,
    invocation: HookInvocation,
    workspace_root: &Path,
    cancel: &CancellationToken,
) -> RuntimeHookDispatch {
    let point = invocation.point();
    let _ = sink
        .send_event(RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
            hook_name: format!("{point:?}"),
            status: RuntimeHookEventStatus::Running,
            matcher: None,
            command: None,
            result: None,
        }))
        .await;
    let outcome = hook_port
        .dispatch_at(invocation, HookDispatchContext::new(workspace_root), cancel)
        .await;
    let dispatch = map_hook_outcome(&outcome);
    let status = if matches!(dispatch.directive, RuntimeHookDirective::Block { .. }) {
        RuntimeHookEventStatus::Blocked
    } else if dispatch.executions.iter().any(|execution| {
        matches!(
            execution.status,
            crate::application::hook::outcome_mapper::RuntimeHookExecutionStatus::ExecutionFailed { .. }
        )
    }) {
        RuntimeHookEventStatus::Failed
    } else {
        RuntimeHookEventStatus::Succeeded
    };
    let _ = sink
        .send_event(RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
            hook_name: format!("{point:?}"),
            status,
            matcher: dispatch
                .messages
                .first()
                .map(|message| message.source.clone()),
            command: None,
            result: None,
        }))
        .await;

    for message in &dispatch.messages {
        let kind = match message.kind {
            RuntimeHookDisplayMessageKind::AdditionalContext => {
                RuntimeHookMessageKind::AdditionalContext
            }
            RuntimeHookDisplayMessageKind::SystemMessage => RuntimeHookMessageKind::SystemMessage,
        };
        let _ = sink
            .send_event(RuntimeStreamEvent::HookMessage(RuntimeHookMessage {
                point: message.point,
                source: message.source.clone(),
                execution_ordinal: message.execution_ordinal,
                attempt: message.attempt,
                kind,
                text: message.text.clone(),
            }))
            .await;
    }

    dispatch
}

pub(crate) async fn project_hook_dispatch<S: ChatEventSink>(
    sink: &S,
    point: hook::HookPoint,
    dispatch: &RuntimeHookDispatch,
) {
    let status = if matches!(dispatch.directive, RuntimeHookDirective::Block { .. }) {
        RuntimeHookEventStatus::Blocked
    } else if dispatch.executions.iter().any(|execution| {
        matches!(
            execution.status,
            crate::application::hook::outcome_mapper::RuntimeHookExecutionStatus::ExecutionFailed { .. }
        )
    }) {
        RuntimeHookEventStatus::Failed
    } else {
        RuntimeHookEventStatus::Succeeded
    };
    let _ = sink
        .send_event(RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
            hook_name: format!("{point:?}"),
            status,
            matcher: dispatch
                .messages
                .first()
                .map(|message| message.source.clone()),
            command: None,
            result: None,
        }))
        .await;

    for message in &dispatch.messages {
        let kind = match message.kind {
            RuntimeHookDisplayMessageKind::AdditionalContext => {
                RuntimeHookMessageKind::AdditionalContext
            }
            RuntimeHookDisplayMessageKind::SystemMessage => RuntimeHookMessageKind::SystemMessage,
        };
        let _ = sink
            .send_event(RuntimeStreamEvent::HookMessage(RuntimeHookMessage {
                point: message.point,
                source: message.source.clone(),
                execution_ordinal: message.execution_ordinal,
                attempt: message.attempt,
                kind,
                text: message.text.clone(),
            }))
            .await;
    }
}

pub(crate) fn dispatch_is_blocking(dispatch: &RuntimeHookDispatch) -> bool {
    matches!(dispatch.directive, RuntimeHookDirective::Block { .. })
}
