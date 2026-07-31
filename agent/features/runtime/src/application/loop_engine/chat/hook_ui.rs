//! Hook dispatch helper — typed Runtime value mapping.

use crate::application::hook::outcome_mapper::{
    map_hook_outcome, RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookDisplayMessageKind,
    RuntimeHookExecution, RuntimeHookExecutionStatus, RuntimeHookReason,
};
use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeHookEvent, RuntimeHookEventStatus, RuntimeHookExecutionResult,
    RuntimeHookMessage, RuntimeHookMessageKind, RuntimeStreamEvent,
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
    let (status, matcher, command, result) = hook_event_completion(&dispatch);
    let _ = sink
        .send_event(RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
            hook_name: format!("{point:?}"),
            status,
            matcher,
            command,
            result,
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
    let (status, matcher, command, result) = hook_event_completion(dispatch);
    let _ = sink
        .send_event(RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
            hook_name: format!("{point:?}"),
            status,
            matcher,
            command,
            result,
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

fn hook_event_completion(
    dispatch: &RuntimeHookDispatch,
) -> (
    RuntimeHookEventStatus,
    Option<String>,
    Option<String>,
    Option<RuntimeHookExecutionResult>,
) {
    let matcher = dispatch
        .messages
        .first()
        .map(|message| message.source.clone());
    match &dispatch.directive {
        RuntimeHookDirective::Block { reason } => {
            let command = dispatch
                .block_detail
                .as_ref()
                .map(|detail| detail.command.clone());
            let execution = dispatch
                .block_detail
                .as_ref()
                .map(|detail| &detail.execution)
                .or_else(|| dispatch.executions.last());
            (
                RuntimeHookEventStatus::Blocked,
                matcher,
                command,
                execution.map(|execution| hook_execution_result(execution, "block", Some(reason))),
            )
        }
        directive => {
            let execution = dispatch.executions.last();
            let status = match execution.map(|execution| &execution.status) {
                Some(RuntimeHookExecutionStatus::ExecutionFailed { .. }) => {
                    RuntimeHookEventStatus::Failed
                }
                _ => RuntimeHookEventStatus::Succeeded,
            };
            let decision = match directive {
                RuntimeHookDirective::Continue => "continue",
                RuntimeHookDirective::Context { .. } => "continue_with_context",
                RuntimeHookDirective::UpdatedInput { .. } => "continue_with_updated_input",
                RuntimeHookDirective::ContextAndInput { .. } => "continue_with_context_and_input",
                RuntimeHookDirective::Block { .. } => unreachable!("block handled above"),
            };
            (
                status,
                matcher,
                None,
                execution.map(|execution| hook_execution_result(execution, decision, None)),
            )
        }
    }
}

fn hook_execution_result(
    execution: &RuntimeHookExecution,
    decision: &str,
    block_reason: Option<&RuntimeHookReason>,
) -> RuntimeHookExecutionResult {
    RuntimeHookExecutionResult {
        exit_code: execution.exit_code,
        stdout: execution.stdout.clone(),
        stderr: execution.stderr.clone(),
        decision: Some(decision.to_string()),
        reason: block_reason
            .map(format_hook_reason)
            .or_else(|| match &execution.status {
                RuntimeHookExecutionStatus::ExecutionFailed { error } => Some(error.clone()),
                RuntimeHookExecutionStatus::Success | RuntimeHookExecutionStatus::Blocked => None,
            }),
        additional_context: None,
    }
}

fn format_hook_reason(reason: &RuntimeHookReason) -> String {
    match reason {
        RuntimeHookReason::ExitCode { code, stderr } => {
            if stderr.trim().is_empty() {
                format!("exit code {code}")
            } else {
                stderr.clone()
            }
        }
        RuntimeHookReason::JsonBlock { reason } => reason.clone(),
        RuntimeHookReason::JsonContinueFalse { stop_reason } => stop_reason
            .clone()
            .unwrap_or_else(|| "hook returned continue:false".to_string()),
        RuntimeHookReason::StopHookExecutionFailed { error }
        | RuntimeHookReason::PolicyBlock { error } => error.clone(),
    }
}

pub(crate) fn dispatch_is_blocking(dispatch: &RuntimeHookDispatch) -> bool {
    matches!(dispatch.directive, RuntimeHookDirective::Block { .. })
}
