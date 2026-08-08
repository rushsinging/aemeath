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
use hook::{HookInvocation, HookPort, PreToolUseInput};
use policy::PolicyPort;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::ToolExecutionContext;
#[cfg(test)]
use tools::ToolExecutionPort;
use tools::ToolOutcome;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls<S>(
    context: &RuntimeRunContext,
    agent_approved: &[PreparedToolCall],
    agent: &crate::application::tool::agent::Agent,
    agent_ctx: &ToolExecutionContext,
    agent_semaphore: &Arc<tokio::sync::Semaphore>,
    workspace_persist: &Arc<dyn project::WorkspacePersist>,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    cancel: &CancellationToken,
    workspace_root: &std::path::Path,
    catalog: &tools::ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
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
            let workspace_root = workspace_root.to_path_buf();
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
                    &workspace_root,
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
    hook_port: Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    agent: &crate::application::tool::agent::Agent,
    agent_tool_context: &mut ToolExecutionContext,
    workspace_persist: &Arc<dyn project::WorkspacePersist>,
    workspace_root: &std::path::Path,
    cancel: &CancellationToken,
    authorization: tools::AuthorizationContext,
    catalog: &tools::ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
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
        HookInvocation::PreToolUse(PreToolUseInput {
            tool_name: call.name.clone(),
            tool_input: call.input.clone(),
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
        workspace_root,
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
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<tools::AgentProgressEvent>(32);
    let prog_adapter = crate::application::run::context::tool_progress_sink(prog_tx);
    *agent_tool_context = agent_tool_context.with_progress(Some(prog_adapter.clone()));
    let call_id = effective_call.id.clone();
    let ui_sink = sink.clone();
    let progress_context = context.clone();
    let child_parent_context = progress_context.clone();
    let child_parent_tool_id = call_id.clone();
    let progress_log_context = logging::capture();
    let forward_handle = logging::spawn_instrumented(progress_log_context, async move {
        let mut child_activity_publisher =
            ChildRunActivityPublisher::new(child_parent_context, child_parent_tool_id);
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
            for activity in child_activity_publisher.publish(event.clone()) {
                let _ = ui_sink
                    .send_event(RuntimeStreamEvent::ChildRunActivity(activity))
                    .await;
            }
            let source_context = event
                .source_context
                .as_ref()
                .map(|source| {
                    RuntimeRunContext::new(
                        sdk::ChatId::from_legacy_or_new(&source.chat_id),
                        sdk::ChatRunId::from_legacy_or_new(&source.run_id),
                    )
                })
                .unwrap_or_else(|| progress_context.clone());
            let _ = ui_sink
                .send_event(RuntimeStreamEvent::AgentProgress {
                    source_context,
                    attachment_context: progress_context.clone(),
                    tool_id: call_id.clone(),
                    event,
                })
                .await;
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
        cancel,
        workspace_root,
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

struct ChildRunActivityPublisher {
    identity: Option<tools::ChildRunIdentity>,
    parent_context: RuntimeRunContext,
    parent_tool_call_id: sdk::ToolCallId,
    sequence: u64,
}

impl ChildRunActivityPublisher {
    fn new(parent_context: RuntimeRunContext, parent_tool_call_id: sdk::ToolCallId) -> Self {
        Self {
            identity: None,
            parent_context,
            parent_tool_call_id,
            sequence: 0,
        }
    }

    fn publish(&mut self, event: tools::AgentProgressEvent) -> Vec<tools::ChildRunActivityEvent> {
        if self.identity.is_none() {
            if let Some(source) = event.source_context.as_ref() {
                self.identity = Some(tools::ChildRunIdentity {
                    agent_id: source.chat_id.clone(),
                    run_id: source.run_id.clone(),
                    parent_run_id: self.parent_context.run_id.to_string(),
                    spawned_by_tool_call_id: self.parent_tool_call_id.to_string(),
                });
            }
        }
        let Some(identity) = self.identity.clone() else {
            return Vec::new();
        };
        child_run_activity_kinds(event.kind)
            .into_iter()
            .map(|kind| {
                self.sequence = self.sequence.saturating_add(1);
                tools::ChildRunActivityEvent {
                    identity: identity.clone(),
                    sequence: self.sequence,
                    kind,
                }
            })
            .collect()
    }
}

fn child_run_activity_kinds(kind: tools::AgentProgressKind) -> Vec<tools::ChildRunActivityKind> {
    match kind {
        tools::AgentProgressKind::Started { .. } => Vec::new(),
        tools::AgentProgressKind::Message { text } => {
            vec![tools::ChildRunActivityKind::Text { text }]
        }
        tools::AgentProgressKind::Thinking { text } => {
            vec![tools::ChildRunActivityKind::Thinking { text }]
        }
        tools::AgentProgressKind::ToolCalls { calls } => calls
            .into_iter()
            .map(|call| tools::ChildRunActivityKind::ToolCall {
                id: call.id,
                name: call.name,
                input: call.input,
            })
            .collect(),
        tools::AgentProgressKind::ToolOutput { tool_name, text } => {
            vec![tools::ChildRunActivityKind::ToolOutput { tool_name, text }]
        }
        tools::AgentProgressKind::ToolResult {
            tool_call_id,
            tool_name,
            output,
            content,
            is_error,
        } => vec![tools::ChildRunActivityKind::ToolResult {
            tool_call_id,
            tool_name,
            output,
            content,
            is_error,
        }],
        tools::AgentProgressKind::Terminal { outcome } => {
            vec![tools::ChildRunActivityKind::Terminal { outcome }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::loop_engine::chat::{EventFuture, RuntimeStreamEvent};
    use async_trait::async_trait;
    use sdk::ids::{ChatId, ChatRunId, ToolCallId};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::sync::{mpsc, Notify};
    use tools::{TypedTool, TypedToolResult};

    #[test]
    fn child_run_activity_projection_preserves_parent_tool_identity_and_kinds() {
        let parent_context =
            RuntimeRunContext::new(ChatId::new("parent-chat"), ChatRunId::new("parent-run"));
        let parent_tool_id = ToolCallId::new("agent-call");
        let event = tools::AgentProgressEvent {
            source_context: Some(tools::AgentProgressSourceContext::new(
                "researcher",
                "child-run",
            )),
            sequence: 7,
            kind: tools::AgentProgressKind::Thinking {
                text: "reasoning".to_string(),
            },
        };

        let mut publisher =
            ChildRunActivityPublisher::new(parent_context.clone(), parent_tool_id.clone());
        let projected = publisher.publish(event);

        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].identity.agent_id, "researcher");
        assert_eq!(projected[0].identity.run_id, "child-run");
        assert_eq!(
            projected[0].identity.parent_run_id,
            parent_context.run_id.to_string()
        );
        assert_eq!(
            projected[0].identity.spawned_by_tool_call_id,
            parent_tool_id.to_string()
        );
        assert_eq!(projected[0].sequence, 1);
        assert!(matches!(
            &projected[0].kind,
            tools::ChildRunActivityKind::Thinking { text } if text == "reasoning"
        ));
    }

    #[test]
    fn child_run_tool_result_preserves_canonical_tool_name() {
        let parent_context =
            RuntimeRunContext::new(ChatId::new("parent-chat"), ChatRunId::new("parent-run"));
        let parent_tool_id = ToolCallId::new("agent-call");
        let mut publisher = ChildRunActivityPublisher::new(parent_context, parent_tool_id);
        let started = tools::AgentProgressEvent {
            source_context: Some(tools::AgentProgressSourceContext::new(
                "researcher",
                "child-run",
            )),
            sequence: 0,
            kind: tools::AgentProgressKind::Started {
                role: Some("researcher".to_string()),
                model: "model".to_string(),
            },
        };
        assert!(publisher.publish(started).is_empty());

        let projected = publisher.publish(tools::AgentProgressEvent {
            source_context: None,
            sequence: 1,
            kind: tools::AgentProgressKind::ToolResult {
                tool_call_id: "skill-call".to_string(),
                tool_name: "Skill".to_string(),
                output: "SKILL_BODY_SENTINEL".to_string(),
                content: serde_json::json!({"name": "using-superpowers"}),
                is_error: false,
            },
        });

        assert!(matches!(
            &projected[0].kind,
            tools::ChildRunActivityKind::ToolResult {
                tool_name,
                output,
                ..
            } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
        ));
    }

    #[test]
    fn child_run_activity_projection_sequences_tool_output_without_source_context() {
        let parent_context =
            RuntimeRunContext::new(ChatId::new("parent-chat"), ChatRunId::new("parent-run"));
        let parent_tool_id = ToolCallId::new("agent-call");
        let mut publisher = ChildRunActivityPublisher::new(parent_context, parent_tool_id);
        let started = tools::AgentProgressEvent {
            source_context: Some(tools::AgentProgressSourceContext::new(
                "researcher",
                "child-run",
            )),
            sequence: 0,
            kind: tools::AgentProgressKind::Started {
                role: Some("researcher".to_string()),
                model: "model".to_string(),
            },
        };
        assert!(publisher.publish(started).is_empty());

        let output = publisher.publish(tools::AgentProgressEvent {
            source_context: None,
            sequence: 1,
            kind: tools::AgentProgressKind::ToolOutput {
                tool_name: "Bash".to_string(),
                text: "hello".to_string(),
            },
        });

        assert_eq!(output.len(), 1);
        assert_eq!(output[0].identity.run_id, "child-run");
        assert_eq!(output[0].sequence, 1);
        assert!(matches!(
            &output[0].kind,
            tools::ChildRunActivityKind::ToolOutput { tool_name, text }
                if tool_name == "Bash" && text == "hello"
        ));
    }

    #[derive(Clone)]
    struct NoopSink;

    impl ChatEventSink for NoopSink {
        fn send_event<'a>(&'a self, _event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async {})
        }

        fn try_send_event(&self, _event: RuntimeStreamEvent) {}
    }

    /// A test HookPort that always returns Continue.
    struct NoOpHookPort;

    #[async_trait]
    impl HookPort for NoOpHookPort {
        async fn dispatch(
            &self,
            _invocation: HookInvocation,
            _cancellation: &dyn hook::CancellationSignal,
        ) -> hook::HookOutcome {
            hook::HookOutcome::proceed()
        }
    }

    struct ActiveGuard(Arc<AtomicUsize>);

    impl Drop for ActiveGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    struct ControlledAgentTool {
        started: mpsc::UnboundedSender<String>,
        gates: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl TypedTool for ControlledAgentTool {
        type Output = Value;

        fn name(&self) -> &str {
            "Agent"
        }

        fn description(&self) -> &str {
            "controlled agent test tool"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({"type":"object"})
        }

        async fn call(
            &self,
            input: Value,
            _ctx: &ToolExecutionContext,
        ) -> TypedToolResult<Self::Output> {
            let label = input["label"].as_str().unwrap().to_string();
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            let _guard = ActiveGuard(self.active.clone());
            let gate = self.gates.lock().unwrap()[&label].clone();
            self.started.send(label.clone()).unwrap();
            gate.notified().await;
            TypedToolResult::success(label.clone(), serde_json::json!({"label": label}))
        }
    }

    struct Harness {
        execution: Arc<dyn ToolExecutionPort>,
        catalog: tools::ToolCatalogSnapshot,
        ctx: ToolExecutionContext,
        started: mpsc::UnboundedReceiver<String>,
        gates: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
        max_active: Arc<AtomicUsize>,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
    }

    fn harness(labels: &[&str], limit: usize) -> Harness {
        let (started_tx, started) = mpsc::unbounded_channel();
        let gates = Arc::new(Mutex::new(
            labels
                .iter()
                .map(|label| ((*label).to_string(), Arc::new(Notify::new())))
                .collect(),
        ));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let factory = tools::composition::TestCatalogExecutionFactory::new();
        factory.register(ControlledAgentTool {
            started: started_tx,
            gates: gates.clone(),
            active,
            max_active: max_active.clone(),
        });
        let cwd = std::env::current_dir().unwrap();
        let ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
            cwd,
            CancellationToken::new(),
        );
        let ports = factory.build(ctx.clone());
        let catalog = ports.catalog();
        Harness {
            execution: ports.execution(),
            catalog,
            ctx,
            started,
            gates,
            max_active,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(limit)),
        }
    }

    fn call(label: &str, index: usize) -> ToolCall {
        ToolCall {
            id: ToolCallId::from_legacy_or_new(&format!("call-{label}")),
            provider_id: format!("provider-{label}"),
            name: "Agent".to_string(),
            index,
            input: serde_json::json!({"label": label}),
        }
    }

    fn notify(gates: &Arc<Mutex<HashMap<String, Arc<Notify>>>>, label: &str) {
        gates.lock().unwrap()[label].notify_one();
    }

    fn spawn_calls(
        execution: Arc<dyn ToolExecutionPort>,
        ctx: ToolExecutionContext,
        calls: Vec<ToolCall>,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
        cancel: CancellationToken,
        catalog: tools::ToolCatalogSnapshot,
    ) -> tokio::task::JoinHandle<Vec<ToolExecution>> {
        tokio::spawn(async move {
            let sink = NoopSink;
            let hook_port: Arc<dyn HookPort> = Arc::new(NoOpHookPort);
            let activities = crate::application::activity::ActivityCoordinator::new(
                sdk::RunId::new_v7(),
                Arc::new(crate::application::activity::SystemActivityClock),
                Arc::new(crate::application::activity::UuidV7ActivityIdSource),
            );
            let prepared = calls
                .into_iter()
                .map(|call| PreparedToolCall {
                    call,
                    authorization: tools::AuthorizationContext::STANDARD,
                })
                .collect::<Vec<_>>();
            let agent = crate::application::tool::agent::Agent {
                catalog: catalog.clone(),
                execution,
                context: crate::application::context::coordination::ContextCoordinator::new(
                    context::adapters::isolated_context("test-session"),
                ),
                session_id: context::domain::SessionId::new("test-session"),
                ctx: ctx.clone(),
                max_tool_concurrency: 1,
                agent_semaphore: agent_semaphore.clone(),
                workspace_persist:
                    crate::application::run::workspace_test_support::workspace_persist(&ctx),
                tool_result_materializer:
                    crate::application::tool::test_support::test_tool_result_materializer(),
                committed_side_effects: Default::default(),
                runtime_cancellation: cancel.clone(),
            };
            let step_tool_context = ctx.with_cancellation(Arc::new(
                crate::application::run::context::RunCancellationScope::from_token(cancel.clone()),
            ));
            execute_agent_calls(
                &RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn")),
                &prepared,
                &agent,
                &step_tool_context,
                &agent_semaphore,
                &crate::application::run::workspace_test_support::workspace_persist(&ctx),
                &sink,
                &hook_port,
                &activities,
                &cancel,
                std::path::Path::new("."),
                &catalog,
                &policy::AllowAllPolicy,
                &sdk::RunId::new_v7(),
                &sdk::RunStepId::new_v7(),
            )
            .await
        })
    }

    #[tokio::test]
    async fn test_agent_window_starts_next_call_when_one_slot_frees() {
        let mut h = harness(&["first", "slow", "next"], 2);
        let handle = spawn_calls(
            h.execution.clone(),
            h.ctx.clone(),
            vec![call("first", 0), call("slow", 1), call("next", 2)],
            h.agent_semaphore.clone(),
            CancellationToken::new(),
            h.catalog.clone(),
        );

        let first_two = [
            h.started.recv().await.unwrap(),
            h.started.recv().await.unwrap(),
        ];
        assert!(first_two.contains(&"first".to_string()));
        assert!(first_two.contains(&"slow".to_string()));
        notify(&h.gates, "first");
        let next = tokio::time::timeout(std::time::Duration::from_secs(2), h.started.recv())
            .await
            .expect("next Agent should start as soon as one permit is free")
            .unwrap();
        assert_eq!(next, "next");

        notify(&h.gates, "slow");
        notify(&h.gates, "next");
        let results = handle.await.unwrap();
        assert_eq!(
            results
                .iter()
                .map(|result| result.provider_id.as_str())
                .collect::<Vec<_>>(),
            vec!["provider-first", "provider-slow", "provider-next"]
        );
        assert_eq!(h.max_active.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_agent_semaphore_is_shared_across_rounds() {
        let mut h = harness(&["one", "two"], 1);
        let first = spawn_calls(
            h.execution.clone(),
            h.ctx.clone(),
            vec![call("one", 0)],
            h.agent_semaphore.clone(),
            CancellationToken::new(),
            h.catalog.clone(),
        );
        assert_eq!(h.started.recv().await.unwrap(), "one");
        let second = spawn_calls(
            h.execution.clone(),
            h.ctx.clone(),
            vec![call("two", 0)],
            h.agent_semaphore.clone(),
            CancellationToken::new(),
            h.catalog.clone(),
        );

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), h.started.recv())
                .await
                .is_err(),
            "second round must wait for the shared Agent permit"
        );
        notify(&h.gates, "one");
        assert_eq!(h.started.recv().await.unwrap(), "two");
        notify(&h.gates, "two");
        first.await.unwrap();
        second.await.unwrap();
        assert_eq!(h.max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn running_agent_call_uses_current_step_cancellation() {
        let mut harness = harness(&["running"], 1);
        let step_cancel = CancellationToken::new();
        let handle = spawn_calls(
            harness.execution.clone(),
            harness.ctx.clone(),
            vec![call("running", 0)],
            harness.agent_semaphore.clone(),
            step_cancel.clone(),
            harness.catalog.clone(),
        );
        assert_eq!(harness.started.recv().await.unwrap(), "running");

        step_cancel.cancel();
        let results = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("当前 Step 取消必须终止运行中的 Agent tool call")
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].outcome.is_error);
        assert!(
            results[0].outcome.text.contains("cancel"),
            "Agent tool terminal 必须保留取消语义: {}",
            results[0].outcome.text
        );
    }

    #[tokio::test]
    async fn test_cancelled_agent_waiter_never_starts() {
        let mut h = harness(&["running", "waiting"], 1);
        let cancel = CancellationToken::new();
        let handle = spawn_calls(
            h.execution.clone(),
            h.ctx.clone(),
            vec![call("running", 0), call("waiting", 1)],
            h.agent_semaphore.clone(),
            cancel.clone(),
            h.catalog.clone(),
        );
        assert_eq!(h.started.recv().await.unwrap(), "running");

        cancel.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), h.started.recv())
                .await
                .is_err(),
            "cancelled Agent waiting for a permit must not start"
        );
        notify(&h.gates, "running");
        let results = handle.await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider_id, "provider-running");
    }
}
