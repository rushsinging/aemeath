use crate::application::activity::ActivityCoordinator;
use crate::application::loop_engine::chat::hook_ui::dispatch_hook;
use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeRunContext, RuntimeStreamEvent, RuntimeToolCallStatus,
};
use crate::application::tool::agent::{Agent, ToolCall, ToolExecution};
use crate::application::tool::coordination::{
    apply_hook_directive_to_tool_call, HookDirectiveOutcome, PreparedToolCall,
};
use hook::{HookInvocation, HookPort, PreToolUseInput, TaskInput};
use policy::PolicyPort;
use std::path::Path;
use std::sync::Arc;
use tools::ToolOutcome;

use super::tools::{log_tool_result, run_post_tool_hooks, send_tool_call_status, send_tool_result};

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_non_agent<S>(
    context: &RuntimeRunContext,
    agent: &Agent,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    non_agent_calls: &[PreparedToolCall],
    language: &str,
    workspace_root: &Path,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    tool_context: &tools::ToolExecutionContext,
    cancel: &tokio_util::sync::CancellationToken,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let other_calls: Vec<&PreparedToolCall> = non_agent_calls
        .iter()
        .filter(|prepared| prepared.call.name != "AskUserQuestion")
        .collect();

    if other_calls.is_empty() {
        return Vec::new();
    }

    if other_calls.len() == 1 {
        if cancel.is_cancelled() {
            return vec![cancelled_result(other_calls[0], language)];
        }
        return execute_one_non_agent(
            context,
            agent,
            sink,
            hook_port,
            activities,
            other_calls[0],
            language,
            workspace_root,
            policy,
            run_id,
            step_id,
            tool_context,
            cancel,
        )
        .await;
    }

    execute_multiple_non_agent(
        context,
        agent,
        sink,
        hook_port,
        activities,
        &other_calls,
        language,
        workspace_root,
        policy,
        run_id,
        step_id,
        tool_context,
        cancel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_multiple_non_agent<S>(
    context: &RuntimeRunContext,
    agent: &Agent,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    other_calls: &[&PreparedToolCall],
    language: &str,
    workspace_root: &Path,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    tool_context: &tools::ToolExecutionContext,
    cancel: &tokio_util::sync::CancellationToken,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let total_len = other_calls.len();
    let mut results: Vec<Option<ToolExecution>> = vec![None; total_len];
    let (concurrent_positions, sequential_positions) = partition_calls(agent, other_calls);

    if !concurrent_positions.is_empty() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(agent.max_tool_concurrency));
        let futures: Vec<_> = concurrent_positions
            .iter()
            .map(|&pos| {
                let call = other_calls[pos];
                let sink = sink.clone();
                let hook_port = hook_port.clone();
                let sem = semaphore.clone();
                let context = context.clone();
                let workspace_root = workspace_root.to_path_buf();
                async move {
                    if cancel.is_cancelled() {
                        return (pos, Vec::new());
                    }
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    let result = execute_one_non_agent(
                        &context,
                        agent,
                        &sink,
                        &hook_port,
                        activities,
                        call,
                        language,
                        &workspace_root,
                        policy,
                        run_id,
                        step_id,
                        tool_context,
                        cancel,
                    )
                    .await;
                    (pos, result)
                }
            })
            .collect();
        for (pos, result_vec) in futures::future::join_all(futures).await {
            if let Some(r) = result_vec.into_iter().next() {
                results[pos] = Some(r);
            } else {
                results[pos] = Some(cancelled_result(other_calls[pos], language));
            }
        }
    }

    for &pos in &sequential_positions {
        let call = other_calls[pos];
        let result_vec = if cancel.is_cancelled() {
            Vec::new()
        } else {
            execute_one_non_agent(
                context,
                agent,
                sink,
                hook_port,
                activities,
                call,
                language,
                workspace_root,
                policy,
                run_id,
                step_id,
                tool_context,
                cancel,
            )
            .await
        };
        if let Some(r) = result_vec.into_iter().next() {
            results[pos] = Some(r);
        } else {
            results[pos] = Some(cancelled_result(call, language));
        }
    }

    results
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            r.unwrap_or_else(|| {
                panic!("execute_non_agent: result slot {i} was not filled — this is a bug")
            })
        })
        .collect()
}

fn partition_calls(agent: &Agent, calls: &[&PreparedToolCall]) -> (Vec<usize>, Vec<usize>) {
    let mut concurrent_positions = Vec::new();
    let mut sequential_positions = Vec::new();
    for (i, call) in calls.iter().enumerate() {
        let is_safe = agent
            .catalog
            .find(&tools::ToolName::new(&call.call.name))
            .is_some_and(|descriptor| descriptor.is_concurrency_safe());
        if is_safe {
            concurrent_positions.push(i);
        } else {
            sequential_positions.push(i);
        }
    }
    (concurrent_positions, sequential_positions)
}

fn cancelled_result(prepared: &PreparedToolCall, language: &str) -> ToolExecution {
    let call = &prepared.call;
    let msg = match language {
        "zh" => "用户已取消",
        _ => "Cancelled by user",
    };
    ToolExecution::new(call, ToolOutcome::error(msg))
}

#[allow(clippy::too_many_arguments)]
async fn execute_one_non_agent<S>(
    context: &RuntimeRunContext,
    agent: &Agent,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    prepared: &PreparedToolCall,
    language: &str,
    workspace_root: &Path,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    tool_context: &tools::ToolExecutionContext,
    cancel: &tokio_util::sync::CancellationToken,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let call = &prepared.call;
    // #1515: 不再构造 PermissionRequest 伪事件——permission hook 的触发
    // 由授权决策流自然产生（allow_all 下无决策 → 无事件），无需授权开关。
    let owned_call = ToolCall {
        id: call.id.clone(),
        provider_id: call.provider_id.clone(),
        name: call.name.clone(),
        index: call.index,
        input: call.input.clone(),
    };
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing start: kind=non_agent tool_name={} runtime_id={} provider_id={} index={} input_len={}",
        owned_call.name,
        owned_call.id,
        owned_call.provider_id,
        owned_call.index,
        owned_call.input.to_string().len(),
    );
    // #1515: PreToolUse 是事件 hook（项目守卫/观测），必须无条件执行；
    // 授权上下文不含 hook 开关（permission hook 由授权决策流触发）。
    let pre_dispatch = dispatch_hook(
        hook_port,
        activities,
        step_id,
        HookInvocation::PreToolUse(PreToolUseInput {
            tool_name: owned_call.name.clone(),
            tool_input: owned_call.input.clone(),
        }),
        workspace_root,
        cancel,
    )
    .await;
    if crate::application::loop_engine::chat::hook_ui::dispatch_is_blocking(&pre_dispatch) {
        let last_exec = pre_dispatch.executions.last();
        let exit_code = last_exec.and_then(|e| e.exit_code);
        let stderr = last_exec.map(|e| e.stderr.as_str()).unwrap_or("");
        log::debug!(target: crate::LOG_TARGET,
            "pretooluse timing blocked: kind=non_agent tool_name={} runtime_id={} provider_id={} exit_code={:?} error_present={}",
            owned_call.name,
            owned_call.id,
            owned_call.provider_id,
            exit_code,
            !stderr.is_empty(),
        );
        let default_blocked = match language {
            "zh" => "被 PreToolUse hook 阻止",
            _ => "Blocked by PreToolUse hook",
        };
        let error_detail = if stderr.is_empty() {
            default_blocked
        } else {
            stderr
        };
        let result = ToolExecution::new(&owned_call, ToolOutcome::error(error_detail));
        send_tool_result(
            sink,
            context,
            &result,
            agent.tool_result_materializer.as_ref(),
            agent.session_id.as_ref(),
        )
        .await;
        return vec![result];
    }
    // Apply the hook directive through the canonical re-validation path (#926).
    // Block is handled above; here we handle UpdatedInput, ContextAndInput,
    // Context, and the error outcomes (InvalidInput, Denied, ApprovalRequired).
    let hook_outcome = apply_hook_directive_to_tool_call(
        &owned_call,
        pre_dispatch.directive,
        &agent.catalog,
        policy,
        run_id,
        step_id,
        workspace_root,
    );
    let (effective_call, effective_authorization, _hook_context) = match hook_outcome {
        HookDirectiveOutcome::Continue { call, context } => (call, prepared.authorization, context),
        HookDirectiveOutcome::Ready {
            call,
            authorization,
            context,
        } => {
            log::debug!(target: crate::LOG_TARGET,
                "pretooluse timing ready: kind=non_agent tool_name={} runtime_id={} provider_id={} input_updated={}",
                call.name,
                call.id,
                call.provider_id,
                call.input != owned_call.input,
            );
            (call, authorization, context)
        }
        HookDirectiveOutcome::InvalidInput { error, .. } => {
            let msg = format!("PreToolUse hook returned invalid input: {error}");
            let result = ToolExecution::new(&owned_call, ToolOutcome::error(msg));
            send_tool_result(
                sink,
                context,
                &result,
                agent.tool_result_materializer.as_ref(),
                agent.session_id.as_ref(),
            )
            .await;
            return vec![result];
        }
        HookDirectiveOutcome::Denied { reason, .. } => {
            let msg = format!("Denied by PreToolUse hook re-evaluation: {reason}");
            let result = ToolExecution::new(&owned_call, ToolOutcome::error(msg));
            send_tool_result(
                sink,
                context,
                &result,
                agent.tool_result_materializer.as_ref(),
                agent.session_id.as_ref(),
            )
            .await;
            return vec![result];
        }
        HookDirectiveOutcome::ApprovalRequired { reason, .. } => {
            let msg = format!("Approval required after PreToolUse hook: {reason}");
            let result = ToolExecution::new(&owned_call, ToolOutcome::error(msg));
            send_tool_result(
                sink,
                context,
                &result,
                agent.tool_result_materializer.as_ref(),
                agent.session_id.as_ref(),
            )
            .await;
            return vec![result];
        }
        HookDirectiveOutcome::Blocked { reason, .. } => {
            // Should have been caught by dispatch_is_blocking above, but
            // be defensive: Block can also reach here if re-validation
            // synthesizes a Blocked.
            let msg = format!("Blocked by PreToolUse hook: {reason:?}");
            let result = ToolExecution::new(&owned_call, ToolOutcome::error(msg));
            send_tool_result(
                sink,
                context,
                &result,
                agent.tool_result_materializer.as_ref(),
                agent.session_id.as_ref(),
            )
            .await;
            return vec![result];
        }
    };
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing approved: kind=non_agent tool_name={} runtime_id={} provider_id={} executions={}",
        effective_call.name,
        effective_call.id,
        effective_call.provider_id,
        pre_dispatch.executions.len(),
    );
    send_tool_call_status(sink, context, &effective_call, RuntimeToolCallStatus::Ready).await;
    send_tool_call_status(
        sink,
        context,
        &effective_call,
        RuntimeToolCallStatus::Running,
    )
    .await;
    log::debug!(target: crate::LOG_TARGET,
        "tool execution timing running_sent: kind=non_agent tool_name={} runtime_id={} provider_id={}",
        effective_call.name,
        effective_call.id,
        effective_call.provider_id,
    );
    // Only Bash supports stdout streaming via progress_tx. For other tools,
    // skip the channel setup to avoid unnecessary overhead.
    let is_bash = effective_call.name == "Bash";

    let tool_ctx = tool_context.with_authorization(effective_authorization);
    log::debug!(
        target: crate::LOG_TARGET,
        "non-agent tool cancellation context bound: run_id={} step_id={} call_id={} tool={} cancelled={}",
        run_id,
        step_id,
        effective_call.id,
        effective_call.name,
        tool_ctx.cancellation().is_cancelled()
    );
    let exec_results = if is_bash {
        // Set up tool stream channel for stdout streaming.
        // Uses ToolProgressEvent (not AgentProgressEvent) since Bash stdout
        // is tool output, not sub-agent progress.
        let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<tools::ToolProgressEvent>(32);
        let streaming_ctx = tool_ctx.with_progress(Some(
            crate::application::run::context::tool_stream_progress_sink(prog_tx),
        ));
        let call_id = effective_call.id.clone();
        let stream_sink = sink.clone();
        let stream_context = context.clone();
        let progress_log_context = logging::capture();
        let forward_handle = logging::spawn_instrumented(progress_log_context, async move {
            while let Some(event) = prog_rx.recv().await {
                let _ = stream_sink
                    .send_event(RuntimeStreamEvent::ToolProgress {
                        context: stream_context.clone(),
                        tool_id: call_id.clone(),
                        event,
                    })
                    .await;
            }
        });

        let results = vec![
            agent
                .execute_one_with_ctx(&effective_call, &streaming_ctx, step_id)
                .await,
        ];

        // Drop the sender so the forwarding task can complete naturally.
        drop(streaming_ctx);

        // Flush any remaining progress events before proceeding.
        // Abort the forwarding task if it doesn't complete within 500ms
        // to prevent task/resource leaks.
        let mut forward_handle = forward_handle;
        tokio::select! {
            _ = &mut forward_handle => {}
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                forward_handle.abort();
                let _ = forward_handle.await;
            }
        }
        results
    } else {
        // Non-Bash tools: execute without progress streaming.
        vec![
            agent
                .execute_one_with_ctx(&effective_call, &tool_ctx, step_id)
                .await,
        ]
    };

    let workspace = agent.workspace_persist.snapshot();
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            workspace_root: workspace.workspace_root.clone(),
            workspace,
        })
        .await;
    let mut out = Vec::new();
    for ex in exec_results {
        let is_error = ex.outcome.is_error;
        log_tool_result(
            &ex.call_id,
            &effective_call.name,
            is_error,
            &ex.outcome.text,
        );
        run_post_tool_hooks(
            hook_port,
            activities,
            step_id,
            &effective_call,
            &ex,
            cancel,
            workspace_root,
        )
        .await;
        run_task_hooks(
            hook_port,
            activities,
            step_id,
            &effective_call,
            &ex.outcome,
            workspace_root,
            cancel,
        )
        .await;
        // Task state 由 loop runner 在 materialization 后依据 typed committed change 统一发布。
        // 不再在此处发 TasksChanged 通知。
        send_tool_result(
            sink,
            context,
            &ex,
            agent.tool_result_materializer.as_ref(),
            agent.session_id.as_ref(),
        )
        .await;
        out.push(ex);
    }
    out
}

async fn run_task_hooks(
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    step_id: &sdk::RunStepId,
    call: &ToolCall,
    outcome: &tools::ToolOutcome,
    workspace_root: &Path,
    cancel: &tokio_util::sync::CancellationToken,
) {
    let Some(task_change) = outcome.task_change.as_ref() else {
        return;
    };
    for fact in task_change.facts() {
        let invocation = match fact {
            tools::TaskChangeFact::Created { .. } => HookInvocation::TaskCreated(TaskInput {
                tool_input: call.input.clone(),
                tool_output: outcome.text.clone(),
            }),
            tools::TaskChangeFact::Completed { .. } => HookInvocation::TaskCompleted(TaskInput {
                tool_input: call.input.clone(),
                tool_output: outcome.text.clone(),
            }),
        };
        let _ = dispatch_hook(
            hook_port,
            activities,
            step_id,
            invocation,
            workspace_root,
            cancel,
        )
        .await;
    }
}

#[cfg(test)]
#[path = "non_agent_tests.rs"]
mod tests;
