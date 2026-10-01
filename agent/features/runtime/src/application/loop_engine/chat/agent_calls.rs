use crate::application::activity::ActivityCoordinator;
use crate::application::loop_engine::chat::hook_ui::dispatch_hook;
use crate::application::loop_engine::chat::tools::{
    run_post_tool_hooks, send_tool_call_status, send_tool_result,
};
use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeRunContext, RuntimeStreamEvent, RuntimeToolCallStatus,
};
use crate::application::tool::agent::{ToolCall, ToolExecution};
use crate::application::tool::coordination::{
    apply_hook_directive_to_tool_call, HookDirectiveOutcome, PreparedToolCall,
};
use hook::{HookDispatcher, HookInvocationData};
use policy::Policy;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::published::execution::ToolExecutionContext;
#[cfg(test)]
use tools::published::execution::ToolExecutionPort;
use tools::ToolOutcome;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls<S>(
    context: &RuntimeRunContext,
    agent_approved: &[PreparedToolCall],
    agent: &crate::application::tool::agent::Agent,
    agent_ctx: &ToolExecutionContext,
    agent_semaphore: &Arc<tokio::sync::Semaphore>,
    workspace_persist: &Arc<dyn project::WorkspaceWriter>,
    sink: &S,
    hook_port: &Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    cancel: &CancellationToken,
    workspace_read: &Arc<dyn project::WorkspaceReader>,
    catalog: &tools::ToolCatalogSnapshot,
    policy: &dyn Policy,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let agent_futures: Vec<_> = agent_approved
        .iter()
        .enumerate()
        .map(|(position, prepared)| {
            let call = prepared.call.clone();
            let authorization = prepared.authorization;
            let sink = sink.clone();
            let hook_port = hook_port.clone();
            let agent_semaphore = agent_semaphore.clone();
            let workspace_persist = workspace_persist.clone();
            let mut agent_tool_context = agent_ctx.clone();
            let context = context.clone();
            let cancel = cancel.clone();
            let workspace_read = workspace_read.clone();
            let catalog = catalog.clone();
            let run_id = run_id.clone();
            let step_id = step_id.clone();
            async move {
                let permit = tokio::select! {
                    permit = agent_semaphore.clone().acquire_owned() => permit.ok(),
                    () = cancel.cancelled() => None,
                }?;
                if cancel.is_cancelled() {
                    return None;
                }
                let results = execute_one_agent(
                    &context,
                    call,
                    sink,
                    hook_port,
                    activities,
                    agent,
                    &mut agent_tool_context,
                    &workspace_persist,
                    &workspace_read,
                    &cancel,
                    authorization,
                    &catalog,
                    policy,
                    &run_id,
                    &step_id,
                )
                .await;
                drop(permit);
                Some((position, results))
            }
        })
        .collect();

    let mut ordered_results: Vec<Option<Vec<ToolExecution>>> = std::iter::repeat_with(|| None)
        .take(agent_approved.len())
        .collect();
    for result in futures::future::join_all(agent_futures)
        .await
        .into_iter()
        .flatten()
    {
        ordered_results[result.0] = Some(result.1);
    }
    ordered_results.into_iter().flatten().flatten().collect()
}

#[allow(clippy::too_many_arguments)]
async fn execute_one_agent<S>(
    context: &RuntimeRunContext,
    call: ToolCall,
    sink: S,
    hook_port: Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    agent: &crate::application::tool::agent::Agent,
    agent_tool_context: &mut ToolExecutionContext,
    workspace_persist: &Arc<dyn project::WorkspaceWriter>,
    workspace_read: &Arc<dyn project::WorkspaceReader>,
    cancel: &CancellationToken,
    authorization: share::tools_vocab::AuthorizationContext,
    catalog: &tools::ToolCatalogSnapshot,
    policy: &dyn Policy,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let workspace_root = workspace_read.current_workspace_root();
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing start: kind=agent tool_name={} runtime_id={} provider_id={} index={} input_len={}",
        call.name,
        call.id,
        call.provider_id,
        call.index,
        call.input.to_string().len(),
    );
    let original_input = call.input.clone();
    // #1515: PreToolUse 是事件 hook（项目守卫/观测），必须无条件执行；
    // 授权上下文不含 hook 开关（permission hook 由授权决策流触发）。
    let pre_dispatch = dispatch_hook(
        &hook_port,
        activities,
        step_id,
        HookInvocationData::PreToolUse {
            tool_name: call.name.clone(),
            tool_input: call.input.clone(),
        },
        &workspace_root,
        agent.session_id.as_ref(),
        cancel,
    )
    .await;
    if crate::application::loop_engine::chat::hook_ui::dispatch_is_blocking(&pre_dispatch) {
        let last_exec = pre_dispatch.executions.last();
        let exit_code = last_exec.and_then(|e| e.exit_code);
        let stderr = last_exec.map(|e| e.stderr.as_str()).unwrap_or("");
        log::debug!(target: crate::LOG_TARGET,
            "pretooluse timing blocked: kind=agent tool_name={} runtime_id={} provider_id={} exit_code={:?} error_present={}",
            call.name,
            call.id,
            call.provider_id,
            exit_code,
            !stderr.is_empty(),
        );
        let error_detail = if stderr.is_empty() {
            "Blocked by PreToolUse hook"
        } else {
            stderr
        };
        let result = ToolExecution::new(&call, ToolOutcome::error(error_detail));
        send_tool_result(
            &sink,
            context,
            &result,
            agent.tool_result_materializer.as_ref(),
            agent.session_id.as_ref(),
        )
        .await;
        return vec![result];
    }
    // Apply the hook directive through the canonical re-validation path (#926).
    let hook_outcome = apply_hook_directive_to_tool_call(
        &call,
        pre_dispatch.directive,
        catalog,
        policy,
        run_id,
        step_id,
        &workspace_root,
    );
    let (effective_call, _effective_authorization, _hook_context) = match hook_outcome {
        HookDirectiveOutcome::Continue { call, context } => (call, authorization, context),
        HookDirectiveOutcome::Ready {
            call,
            authorization,
            context,
        } => {
            log::debug!(target: crate::LOG_TARGET,
                "pretooluse timing ready: kind=agent tool_name={} runtime_id={} provider_id={} input_updated={}",
                call.name,
                call.id,
                call.provider_id,
                call.input != original_input,
            );
            (call, authorization, context)
        }
        HookDirectiveOutcome::InvalidInput { error, .. } => {
            let msg = format!("PreToolUse hook returned invalid input: {error}");
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(
                &sink,
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
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(
                &sink,
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
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(
                &sink,
                context,
                &result,
                agent.tool_result_materializer.as_ref(),
                agent.session_id.as_ref(),
            )
            .await;
            return vec![result];
        }
        HookDirectiveOutcome::Blocked { reason, .. } => {
            let msg = format!("Blocked by PreToolUse hook: {reason:?}");
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(
                &sink,
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
        "pretooluse timing approved: kind=agent tool_name={} runtime_id={} provider_id={} executions={}",
        effective_call.name,
        effective_call.id,
        effective_call.provider_id,
        pre_dispatch.executions.len(),
    );
    send_tool_call_status(
        &sink,
        context,
        &effective_call,
        RuntimeToolCallStatus::Ready,
    )
    .await;
    send_tool_call_status(
        &sink,
        context,
        &effective_call,
        RuntimeToolCallStatus::Running,
    )
    .await;
    log::debug!(target: crate::LOG_TARGET,
        "tool execution timing running_sent: kind=agent tool_name={} runtime_id={} provider_id={}",
        effective_call.name,
        effective_call.id,
        effective_call.provider_id,
    );

    log::debug!(
        target: crate::LOG_TARGET,
        "agent tool cancellation context bound: run_id={} step_id={} call_id={} tool={} cancelled={}",
        run_id,
        step_id,
        effective_call.id,
        effective_call.name,
        agent_tool_context.cancellation().is_cancelled()
    );
    let (prog_tx, mut prog_rx) =
        tokio::sync::mpsc::channel::<tools::published::agent::AgentProgressEvent>(32);
    let prog_adapter = crate::application::run::context::tool_progress_sink(prog_tx);
    *agent_tool_context = agent_tool_context.with_progress(Some(prog_adapter.clone()));
    let call_id = effective_call.id.clone();
    let ui_sink = sink.clone();
    let progress_context = context.clone();
    let child_parent_context = progress_context.clone();
    let child_parent_tool_id = call_id.clone();
    let progress_log_context = logging::capture();
    let forward_handle = logging::spawn_instrumented(progress_log_context, async move {
        let mut sub_run_fact_publisher =
            SubRunFactPublisher::new(child_parent_context, child_parent_tool_id);
        while let Some(event) = prog_rx.recv().await {
            log::debug!(
                target: crate::LOG_TARGET,
                "[agent_progress_forward] tool_id={} kind={} seq={} source_chat_id={} source_run_id={} attachment_chat_id={} attachment_run_id={}",
                call_id.as_str(),
                format!("{:?}", event.kind).split('{').next().unwrap_or("?"),
                event.sequence,
                event.source_context.as_ref().map(|source| source.chat_id.as_str()).unwrap_or("<attachment>"),
                event.source_context.as_ref().map(|source| source.run_id.as_str()).unwrap_or("<attachment>"),
                progress_context.chat_id,
                progress_context.run_id,
            );
            for fact in sub_run_fact_publisher.publish(event) {
                let runtime_event = match fact {
                    SubRunPublishedFact::Started(started) => {
                        RuntimeStreamEvent::SubRunStarted(started)
                    }
                    SubRunPublishedFact::Activity(activity) => {
                        RuntimeStreamEvent::SubRunActivity(activity)
                    }
                };
                let _ = ui_sink.send_event(runtime_event).await;
            }
        }
    });

    let execution = agent
        .execute_one_with_ctx(&effective_call, agent_tool_context, step_id)
        .await;
    let workspace = workspace_persist.snapshot();
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            workspace_root: workspace.workspace_root.clone(),
            workspace,
        })
        .await;
    *agent_tool_context = agent_tool_context.with_progress(None);
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    run_post_tool_hooks(
        &hook_port,
        activities,
        step_id,
        &effective_call,
        &execution,
        agent.session_id.as_ref(),
        cancel,
        workspace_read,
    )
    .await;
    send_tool_result(
        &sink,
        context,
        &execution,
        agent.tool_result_materializer.as_ref(),
        agent.session_id.as_ref(),
    )
    .await;
    vec![execution]
}

enum SubRunPublishedFact {
    Started(tools::published::sub_run::SubRunStartedEvent),
    Activity(tools::published::sub_run::SubRunActivityEvent),
}

struct SubRunFactPublisher {
    identity: Option<tools::published::sub_run::SubRunIdentity>,
    parent_context: RuntimeRunContext,
    parent_tool_call_id: sdk::ToolCallId,
    sequence: u64,
}

impl SubRunFactPublisher {
    fn new(parent_context: RuntimeRunContext, parent_tool_call_id: sdk::ToolCallId) -> Self {
        Self {
            identity: None,
            parent_context,
            parent_tool_call_id,
            sequence: 0,
        }
    }

    fn publish(
        &mut self,
        event: tools::published::agent::AgentProgressEvent,
    ) -> Vec<SubRunPublishedFact> {
        if self.identity.is_none() {
            if let Some(source) = event.source_context.as_ref() {
                self.identity = Some(tools::published::sub_run::SubRunIdentity {
                    agent_id: source.chat_id.clone(),
                    run_id: source.run_id.clone(),
                    parent_chat_id: self.parent_context.chat_id.to_string(),
                    parent_run_id: self.parent_context.run_id.to_string(),
                    spawned_by_tool_call_id: self.parent_tool_call_id.to_string(),
                });
            }
        }
        let Some(identity) = self.identity.clone() else {
            return Vec::new();
        };
        match event.kind {
            tools::published::agent::AgentProgressKind::Started { role, model } => {
                self.sequence = self.sequence.saturating_add(1);
                vec![SubRunPublishedFact::Started(
                    tools::published::sub_run::SubRunStartedEvent {
                        identity,
                        sequence: self.sequence,
                        role,
                        model,
                    },
                )]
            }
            kind => sub_run_activity_kinds(kind)
                .into_iter()
                .map(|kind| {
                    self.sequence = self.sequence.saturating_add(1);
                    SubRunPublishedFact::Activity(tools::published::sub_run::SubRunActivityEvent {
                        identity: identity.clone(),
                        sequence: self.sequence,
                        kind,
                    })
                })
                .collect(),
        }
    }
}

fn sub_run_activity_kinds(
    kind: tools::published::agent::AgentProgressKind,
) -> Vec<tools::published::sub_run::SubRunActivityKind> {
    match kind {
        tools::published::agent::AgentProgressKind::Started { .. } => Vec::new(),
        tools::published::agent::AgentProgressKind::Message { text } => {
            vec![tools::published::sub_run::SubRunActivityKind::Text { text }]
        }
        tools::published::agent::AgentProgressKind::Thinking { text } => {
            vec![tools::published::sub_run::SubRunActivityKind::Thinking { text }]
        }
        tools::published::agent::AgentProgressKind::ToolCalls { calls } => calls
            .into_iter()
            .map(
                |call| tools::published::sub_run::SubRunActivityKind::ToolCall {
                    id: call.id,
                    name: call.name,
                    input: call.input,
                },
            )
            .collect(),
        tools::published::agent::AgentProgressKind::ToolOutput { tool_name, text } => {
            vec![tools::published::sub_run::SubRunActivityKind::ToolOutput { tool_name, text }]
        }
        tools::published::agent::AgentProgressKind::ToolResult {
            tool_call_id,
            tool_name,
            output,
            content,
            is_error,
        } => vec![tools::published::sub_run::SubRunActivityKind::ToolResult {
            tool_call_id,
            tool_name,
            output,
            content,
            is_error,
        }],
        tools::published::agent::AgentProgressKind::Terminal { outcome } => {
            vec![tools::published::sub_run::SubRunActivityKind::Terminal { outcome }]
        }
    }
}

#[cfg(test)]
#[path = "agent_calls_tests.rs"]
mod tests;
