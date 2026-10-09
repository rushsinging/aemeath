use crate::application::activity::ActivityCoordinator;
use crate::application::loop_engine::chat::agent_calls::execute_agent_calls;
use crate::application::loop_engine::chat::hook_ui::dispatch_hook;
use crate::application::loop_engine::chat::non_agent::execute_non_agent;
use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeRunContext, RuntimeStreamEvent, RuntimeToolCallStatus,
};
use crate::application::loop_engine::{ApprovalRequiredCall, SuspendedQuestion, SuspendedToolCall};
use crate::application::tool::agent::{Agent, ToolCall, ToolExecution};
use crate::application::tool::coordination::{prepare_tool_round, restore_tool_call_order};
use hook::{HookDispatcher, HookInvocationData};

use sdk::ids::ToolCallId;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::published::execution::ToolSuspension;
use tools::ToolOutcome;

/// Result of a tool execution round.
/// Suspensions and approvals are returned as typed data — the caller
/// The active loop capability adapter decides whether to route them through
/// the interaction coordinator.
pub(crate) struct ToolRoundResult {
    pub results: Vec<ToolExecution>,
    pub fuse_bypassed: Vec<ToolCallId>,
    /// Tool calls that produced suspensions (AskUserQuestion).
    pub suspensions: Vec<SuspendedToolCall>,
    /// Tool calls that need approval (denied by policy with approval possible).
    pub approvals: Vec<ApprovalRequiredCall>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tool_round<S>(
    context: &RuntimeRunContext,
    tool_calls: &[ToolCall],
    catalog: &tools::ToolCatalogSnapshot,
    policy: &dyn policy::Policy,
    triage: Option<&policy::PolicyTriage>,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    agent: &Agent,
    sink: &S,
    hook_port: &Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    cancel: &CancellationToken,
    language: &str,
    workspace_read: &Arc<dyn project::WorkspaceReader>,
    guarded_calls: &[(ToolCall, crate::application::loop_engine::ToolGuardDecision)],
) -> ToolRoundResult
where
    S: ChatEventSink,
{
    let workspace_root = workspace_read.current_workspace_root();
    let prepared = prepare_tool_round(
        guarded_calls,
        catalog,
        policy,
        triage,
        run_id,
        step_id,
        &workspace_root,
    )
    .await;
    let denied_results = deny_tool_calls(
        &prepared.denied,
        sink,
        context,
        hook_port,
        activities,
        step_id,
        cancel,
        &workspace_root,
        agent,
    )
    .await;
    let fuse_bypassed = prepared.fuse_bypassed.clone();
    let approved = prepared.executable;
    let fused_results =
        publish_guard_blocked(prepared.guard_blocked, tool_calls, sink, context, agent).await;

    let (agent_approved, non_agent_approved): (Vec<_>, Vec<_>) = approved
        .into_iter()
        .partition(|prepared| prepared.call.name == "Agent");

    let step_tool_context = agent.ctx.with_cancellation(Arc::new(
        crate::application::run::context::RunCancellationScope::from_token(cancel.clone()),
    ));
    log::debug!(
        target: crate::LOG_TARGET,
        "tool round cancellation context bound: run_id={} step_id={} cancelled={}",
        run_id,
        step_id,
        step_tool_context.cancellation().is_cancelled()
    );

    // Execute AskUserQuestion calls through the tool execution port.
    // Suspensions are collected and returned to the caller — they are NOT
    // resolved inline. The caller routes them through the engine's
    // interaction coordinator.
    let mut suspensions: Vec<SuspendedToolCall> = Vec::new();
    let mut ask_user_terminal = Vec::new();
    for prepared in non_agent_approved
        .iter()
        .filter(|prepared| prepared.call.name == "AskUserQuestion")
    {
        let call = &prepared.call;
        let tool_ctx = step_tool_context.with_authorization(prepared.authorization);
        match agent
            .execute_one_outcome_with_ctx(call, &tool_ctx, step_id)
            .await
        {
            (tools::published::execution::ToolExecutionOutcome::Suspended(suspension), _) => {
                let questions = match suspension {
                    ToolSuspension::UserInteraction(spec) => spec
                        .questions
                        .iter()
                        .map(|q| SuspendedQuestion {
                            prompt: q.prompt.clone(),
                            options: q
                                .options
                                .iter()
                                .map(|o| {
                                    sdk::OptionItem::new(o.title.clone(), o.description.clone())
                                })
                                .collect(),
                            allow_multi: q.allow_multi,
                        })
                        .collect(),
                };
                suspensions.push(SuspendedToolCall {
                    call: (*call).clone(),
                    questions,
                });
            }
            (outcome, duration_ms) => ask_user_terminal.push(
                ToolExecution::new(
                    call,
                    crate::application::tool::agent::legacy_outcome(outcome),
                )
                .with_optional_duration(duration_ms),
            ),
        }
    }
    let non_agent_results = execute_non_agent(
        context,
        agent,
        sink,
        hook_port,
        activities,
        &non_agent_approved,
        language,
        workspace_read,
        policy,
        run_id,
        step_id,
        &step_tool_context,
        cancel,
    )
    .await;
    let agent_results = execute_agent_calls(
        context,
        &agent_approved,
        agent,
        &step_tool_context,
        &agent.agent_semaphore,
        &agent.workspace_persist,
        sink,
        hook_port,
        activities,
        cancel,
        workspace_read,
        catalog,
        policy,
        run_id,
        step_id,
    )
    .await;

    let results = ask_user_terminal
        .into_iter()
        .chain(non_agent_results)
        .chain(agent_results)
        .chain(fused_results)
        .chain(denied_results)
        .collect();
    // #1248 TaskData 5: Map RequireApproval calls from policy to engine-level ApprovalRequiredCall.
    let approvals: Vec<ApprovalRequiredCall> = prepared
        .require_approval
        .into_iter()
        .map(|ra| ApprovalRequiredCall {
            call: ra.call,
            authorization: ra.authorization,
            reason: ra.reason,
            subject: ra.subject,
        })
        .collect();
    ToolRoundResult {
        results: restore_tool_call_order(tool_calls, results),
        fuse_bypassed,
        suspensions,
        approvals,
    }
}

async fn publish_guard_blocked<S>(
    blocked: Vec<ToolExecution>,
    calls: &[ToolCall],
    sink: &S,
    context: &RuntimeRunContext,
    agent: &Agent,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    for execution in &blocked {
        let Some(call) = calls.iter().find(|call| call.id == execution.call_id) else {
            continue;
        };
        send_tool_call_status(sink, context, call, RuntimeToolCallStatus::Ready).await;
        send_tool_call_status(sink, context, call, RuntimeToolCallStatus::Running).await;
        send_tool_result(
            sink,
            context,
            execution,
            agent.tool_result_materializer.as_ref(),
            agent.session_id.as_ref(),
        )
        .await;
    }
    blocked
}

#[allow(clippy::too_many_arguments)]
async fn deny_tool_calls<S>(
    denied: &[crate::application::tool::coordination::DeniedToolCall],
    sink: &S,
    context: &RuntimeRunContext,
    hook_port: &Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    step_id: &sdk::RunStepId,
    cancel: &CancellationToken,
    workspace_root: &std::path::Path,
    agent: &Agent,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let mut denied_results = Vec::new();
    for call in denied {
        log::warn!(
            target: crate::LOG_TARGET,
            "tool call denied by policy: name={}, reason={}, runtime_id={}, provider_id={}",
            call.call.name, call.reason, call.call.id, call.call.provider_id,
        );
        let _ = dispatch_hook(
            hook_port,
            activities,
            step_id,
            HookInvocationData::PermissionDenied {
                tool_name: call.call.name.clone(),
                permission_rule: "deny".to_string(),
            },
            workspace_root,
            agent.session_id.as_ref(),
            cancel,
        )
        .await;
        // 发送 ToolCall 事件，让 pending 占位行获取 LLM 的 tool_use_id，
        // 后续 ToolResult 中的 mark_tool_header_done 才能精确匹配（Bug #52）。
        let call_id = call.call.id.clone();
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCallStateChanged {
                context: context.clone(),
                id: call_id.clone(),
                provider_id: Some(call.call.provider_id.clone()),
                name: call.call.name.clone(),
                index: call.call.index,
                arguments: None,
                status: RuntimeToolCallStatus::Ready,
            })
            .await;
        // 保持原 wire 形态 {"status":"error","message":...}（与 deny 路径历史一致）。
        let outcome = ToolOutcome {
            text: call.reason.clone(),
            data: serde_json::json!({
                "status": "error",
                "message": call.reason,
            }),
            is_error: true,
            images: Vec::new(),
            task_change: None,
        };
        let execution = ToolExecution::from_parts(
            call_id,
            call.call.provider_id.clone(),
            call.call.name.clone(),
            outcome,
        );
        send_tool_result(
            sink,
            context,
            &execution,
            agent.tool_result_materializer.as_ref(),
            agent.session_id.as_ref(),
        )
        .await;
        denied_results.push(execution);
    }
    denied_results
}

pub(crate) async fn run_post_tool_hooks(
    hook_port: &Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    step_id: &sdk::RunStepId,
    call: &ToolCall,
    execution: &ToolExecution,
    session_id: &str,
    cancel: &CancellationToken,
    workspace_read: &Arc<dyn project::WorkspaceReader>,
) {
    let workspace_root = workspace_read.current_workspace_root();
    let output = &execution.outcome.text;
    let is_error = execution.outcome.is_error;

    let _ = dispatch_hook(
        hook_port,
        activities,
        step_id,
        HookInvocationData::PostToolUse {
            tool_name: call.name.clone(),
            tool_input: call.input.clone(),
            tool_output: output.to_string(),
            is_error,
        },
        &workspace_root,
        session_id,
        cancel,
    )
    .await;

    if is_error {
        let _ = dispatch_hook(
            hook_port,
            activities,
            step_id,
            HookInvocationData::PostToolUseFailure {
                tool_name: call.name.clone(),
                tool_input: call.input.clone(),
                error: output.to_string(),
            },
            &workspace_root,
            session_id,
            cancel,
        )
        .await;
    }
}

pub(crate) async fn send_tool_call_status<S>(
    sink: &S,
    context: &RuntimeRunContext,
    call: &ToolCall,
    status: RuntimeToolCallStatus,
) where
    S: ChatEventSink,
{
    let _ = sink
        .send_event(RuntimeStreamEvent::ToolCallStateChanged {
            context: context.clone(),
            id: call.id.clone(),
            provider_id: Some(call.provider_id.clone()),
            name: call.name.clone(),
            index: call.index,
            arguments: Some(call.input.clone()),
            status,
        })
        .await;
}

pub(crate) async fn send_tool_result<S>(
    sink: &S,
    context: &RuntimeRunContext,
    execution: &ToolExecution,
    materializer: &crate::application::tool::tool_result_materializer::ToolResultMaterializer,
    session_id: &str,
) where
    S: ChatEventSink,
{
    let (output, content) = materializer
        .materialize_display_result(
            session_id,
            &execution.provider_id,
            &execution.outcome.text,
            &execution.outcome.data,
        )
        .await;
    let _ = sink
        .send_event(RuntimeStreamEvent::ToolResult {
            context: context.clone(),
            id: execution.call_id.clone(),
            provider_id: execution.provider_id.clone(),
            tool_name: execution.tool_name.clone(),
            output,
            content,
            is_error: execution.outcome.is_error,
            images: execution.outcome.images.clone(),
            // #1666：supervisor 测量值经事件流出站（非 supervisor 路径为 None）。
            duration_ms: execution.duration_ms,
        })
        .await;
}

pub(crate) fn log_tool_result(id: &ToolCallId, tool_name: &str, is_error: bool, output: &str) {
    let data = crate::application::loop_engine::llm_log::build_named_tool_result_log(
        id, tool_name, output, is_error, "main",
    );
    log::debug!(
        target: crate::LOG_TARGET,
        "tool_result: {}",
        serde_json::to_string(&data).unwrap_or_default()
    );
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
