use crate::application::agent::{ToolCall, ToolExecution};
use crate::application::chat::looping::hook_ui::dispatch_hook;
use crate::application::chat::looping::tools::{
    run_post_tool_hooks, send_tool_call_status, send_tool_result,
};
use crate::application::chat::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use crate::application::tool_coordination::{
    apply_hook_directive_to_tool_call, HookDirectiveOutcome, PreparedToolCall,
};
use hook::{HookInvocation, HookPort, PreToolUseInput};
use policy::PolicyPort;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::ToolOutcome;
use tools::{ToolExecutionContext, ToolExecutionPort};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls<S>(
    context: &RuntimeTurnContext,
    agent_approved: &[PreparedToolCall],
    execution: &Arc<dyn ToolExecutionPort>,
    agent_ctx: &ToolExecutionContext,
    agent_semaphore: &Arc<tokio::sync::Semaphore>,
    workspace_persist: &Arc<dyn project::WorkspacePersist>,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
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
            let execution_ref = execution.clone();
            let agent_semaphore = agent_semaphore.clone();
            let workspace_persist = workspace_persist.clone();
            let mut ag_ctx = agent_ctx.clone();
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
                    execution_ref,
                    &mut ag_ctx,
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
    context: &RuntimeTurnContext,
    call: ToolCall,
    sink: S,
    hook_port: Arc<dyn HookPort>,
    execution: Arc<dyn ToolExecutionPort>,
    ag_ctx: &mut ToolExecutionContext,
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
    let pre_dispatch = if authorization.enforce_permission_hooks {
        dispatch_hook(
            &hook_port,
            &sink,
            HookInvocation::PreToolUse(PreToolUseInput {
                tool_name: call.name.clone(),
                tool_input: call.input.clone(),
            }),
            workspace_root,
            cancel,
        )
        .await
    } else {
        crate::application::hook_adapter::RuntimeHookDispatch {
            directive: crate::application::hook_adapter::RuntimeHookDirective::Continue,
            executions: Vec::new(),
            messages: Vec::new(),
            block_detail: None,
        }
    };
    if crate::application::chat::looping::hook_ui::dispatch_is_blocking(&pre_dispatch) {
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
        send_tool_result(&sink, context, &result).await;
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
    let (effective_call, effective_authorization, _hook_context) = match hook_outcome {
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
            send_tool_result(&sink, context, &result).await;
            return vec![result];
        }
        HookDirectiveOutcome::Denied { reason, .. } => {
            let msg = format!("Denied by PreToolUse hook re-evaluation: {reason}");
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(&sink, context, &result).await;
            return vec![result];
        }
        HookDirectiveOutcome::ApprovalRequired { reason, .. } => {
            let msg = format!("Approval required after PreToolUse hook: {reason}");
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(&sink, context, &result).await;
            return vec![result];
        }
        HookDirectiveOutcome::Blocked { reason, .. } => {
            let msg = format!("Blocked by PreToolUse hook: {reason:?}");
            let result = ToolExecution::new(&call, ToolOutcome::error(msg));
            send_tool_result(&sink, context, &result).await;
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

    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<tools::AgentProgressEvent>(32);
    *ag_ctx = ag_ctx.with_progress(Some(crate::application::tool_execution_adapters::progress(
        prog_tx,
    )));
    let call_id = effective_call.id.clone();
    let ui_sink = sink.clone();
    let progress_context = context.clone();
    let progress_log_context = logging::capture();
    let forward_handle = logging::spawn_instrumented(progress_log_context, async move {
        while let Some(event) = prog_rx.recv().await {
            let _ = ui_sink
                .send_event(RuntimeStreamEvent::AgentProgress {
                    context: progress_context.clone(),
                    tool_id: call_id.clone(),
                    event,
                })
                .await;
        }
    });

    let cancellation = ag_ctx.cancellation();
    let outcome = execution
        .execute(
            tools::ToolInvocation::new(
                "Agent",
                effective_call.input.clone(),
                ag_ctx.scope().clone(),
            )
            .with_authorization(effective_authorization),
            cancellation.as_ref(),
        )
        .await;
    let workspace = workspace_persist.snapshot();
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            workspace_root: workspace.workspace_root.clone(),
            workspace,
        })
        .await;
    let execution = ToolExecution::new(
        &effective_call,
        crate::application::agent::agent::legacy_outcome(outcome),
    );
    *ag_ctx = ag_ctx.with_progress(None);
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    run_post_tool_hooks(
        &sink,
        &hook_port,
        &effective_call,
        &execution,
        cancel,
        workspace_root,
    )
    .await;
    send_tool_result(&sink, context, &execution).await;
    vec![execution]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::chat::looping::{EventFuture, RuntimeStreamEvent};
    use async_trait::async_trait;
    use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::sync::{mpsc, Notify};
    use tools::{TypedTool, TypedToolResult};

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
            _cancellation: &CancellationToken,
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
        let ctx =
            crate::application::testing::test_tool_execution_context(cwd, CancellationToken::new());
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
            let prepared = calls
                .into_iter()
                .map(|call| PreparedToolCall {
                    call,
                    authorization: tools::AuthorizationContext::STANDARD,
                })
                .collect::<Vec<_>>();
            execute_agent_calls(
                &RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn")),
                &prepared,
                &execution,
                &ctx,
                &agent_semaphore,
                &crate::application::testing::workspace_persist(&ctx),
                &sink,
                &hook_port,
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
