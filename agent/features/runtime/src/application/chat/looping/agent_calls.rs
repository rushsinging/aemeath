use crate::application::agent::{ToolCall, ToolExecution};
use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::tools::{
    run_post_tool_hooks, send_tool_call_status, send_tool_result,
};
use crate::application::chat::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use hook::api::{HookData, ToolHookData};
use share::config::hooks::HookEvent;
use share::tool::ToolOutcome;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::api::{ToolExecutionContext, ToolRegistry};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls<S>(
    context: &RuntimeTurnContext,
    agent_approved: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    agent_ctx: &ToolExecutionContext,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    cancel: &CancellationToken,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let agent_futures: Vec<_> = agent_approved
        .iter()
        .enumerate()
        .map(|(position, call)| {
            let call = ToolCall {
                id: call.id.clone(),
                provider_id: call.provider_id.clone(),
                name: call.name.clone(),
                index: call.index,
                input: call.input.clone(),
            };
            let sink = sink.clone();
            let hook_ui = hook_ui.clone();
            let mut ag_ctx = agent_ctx.clone();
            let hook_runner = hook_runner.clone();
            let registry_ref = registry.clone();
            let context = context.clone();
            let cancel = cancel.clone();
            let workspace_root = workspace_root.to_path_buf();
            async move {
                let permit = tokio::select! {
                    permit = ag_ctx.agent_semaphore.clone().acquire_owned() => permit.ok(),
                    () = cancel.cancelled() => None,
                }?;
                if cancel.is_cancelled() {
                    return None;
                }
                let results = execute_one_agent(
                    &context,
                    call,
                    sink,
                    hook_ui,
                    hook_runner,
                    registry_ref,
                    &mut ag_ctx,
                    &workspace_root,
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
    hook_ui: HookUi<S>,
    hook_runner: hook::api::HookRunner,
    registry: Arc<ToolRegistry>,
    ag_ctx: &mut ToolExecutionContext,
    workspace_root: &Path,
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
    let pre_results = hook_ui
        .run_plain(
            &hook_runner,
            HookEvent::PreToolUse,
            Some(&call.name),
            HookData::Tool(ToolHookData {
                tool_name: call.name.clone(),
                tool_input: call.input.clone(),
                tool_output: None,
                is_error: None,
            }),
            workspace_root,
        )
        .await;
    if let Some(blocked_result) = pre_results.iter().find(|r| r.blocked) {
        log::debug!(target: crate::LOG_TARGET,
            "pretooluse timing blocked: kind=agent tool_name={} runtime_id={} provider_id={} exit_code={:?} error_present={}",
            call.name,
            call.id,
            call.provider_id,
            blocked_result.exit_code,
            blocked_result.error.as_ref().is_some_and(|value| !value.is_empty()),
        );
        let error_detail = blocked_result
            .error
            .as_deref()
            .unwrap_or("Blocked by PreToolUse hook");
        let result = ToolExecution::new(&call, ToolOutcome::error(error_detail));
        send_tool_result(&sink, context, &result).await;
        return vec![result];
    }
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing approved: kind=agent tool_name={} runtime_id={} provider_id={} hook_count={}",
        call.name,
        call.id,
        call.provider_id,
        pre_results.len(),
    );
    send_tool_call_status(&sink, context, &call, RuntimeToolCallStatus::Ready).await;
    send_tool_call_status(&sink, context, &call, RuntimeToolCallStatus::Running).await;
    log::debug!(target: crate::LOG_TARGET,
        "tool execution timing running_sent: kind=agent tool_name={} runtime_id={} provider_id={}",
        call.name,
        call.id,
        call.provider_id,
    );

    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<share::tool::AgentProgressEvent>(32);
    ag_ctx.progress_tx = Some(prog_tx);
    let call_id = call.id.clone();
    let ui_sink = sink.clone();
    let progress_context = context.clone();
    let forward_handle = tokio::spawn(async move {
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

    let agent_tool = registry
        .get("Agent")
        .expect("Agent tool not found in registry");
    let result = agent_tool.call(call.input.clone(), ag_ctx).await;
    let workspace = project::WorkspacePersist::snapshot(ag_ctx.workspace.as_ref());
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            workspace_root: workspace.workspace_root.clone(),
            workspace,
        })
        .await;
    let execution = ToolExecution::new(&call, ToolOutcome::from_tool_result(result));
    ag_ctx.progress_tx = None;
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    run_post_tool_hooks(
        &sink,
        &hook_ui,
        &hook_runner,
        &call,
        &execution,
        workspace_root,
        ag_ctx,
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
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::sync::{mpsc, Notify};
    use tools::api::{ToolResources, TypedTool, TypedToolResult};

    #[derive(Clone)]
    struct NoopSink;

    impl ChatEventSink for NoopSink {
        fn send_event<'a>(&'a self, _event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async {})
        }

        fn try_send_event(&self, _event: RuntimeStreamEvent) {}
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
        registry: Arc<ToolRegistry>,
        ctx: ToolExecutionContext,
        started: mpsc::UnboundedReceiver<String>,
        gates: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
        max_active: Arc<AtomicUsize>,
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
        let registry = Arc::new(ToolRegistry::new());
        registry.register(ControlledAgentTool {
            started: started_tx,
            gates: gates.clone(),
            active,
            max_active: max_active.clone(),
        });
        let cwd = std::env::current_dir().unwrap();
        let ctx = ToolExecutionContext {
            resources: ToolResources {
                agent_runner: None,
                registry: None,
                memory_config: share::config::MemoryConfig::default(),
                lang: "en".to_string(),
                allow_all: true,
            },
            workspace: project::WorkspaceService::new(cwd),
            run_id: sdk::RunId::new_v7().to_string(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            session_reminders: None,
            plan_mode: None,
            max_tool_concurrency: 10,
            max_agent_concurrency: limit,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(limit)),
            progress_tx: None,
            parent_session_id: None,
        };
        Harness {
            registry,
            ctx,
            started,
            gates,
            max_active,
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
        registry: Arc<ToolRegistry>,
        ctx: ToolExecutionContext,
        calls: Vec<ToolCall>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<Vec<ToolExecution>> {
        tokio::spawn(async move {
            let sink = NoopSink;
            let hook_ui = HookUi::new(sink.clone());
            execute_agent_calls(
                &RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn")),
                &calls,
                &registry,
                &ctx,
                &sink,
                &hook_ui,
                &hook::api::HookRunner::new(Default::default()),
                &cancel,
                &std::env::current_dir().unwrap(),
            )
            .await
        })
    }

    #[tokio::test]
    async fn test_agent_window_starts_next_call_when_one_slot_frees() {
        let mut h = harness(&["first", "slow", "next"], 2);
        let handle = spawn_calls(
            h.registry.clone(),
            h.ctx.clone(),
            vec![call("first", 0), call("slow", 1), call("next", 2)],
            CancellationToken::new(),
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
            h.registry.clone(),
            h.ctx.clone(),
            vec![call("one", 0)],
            CancellationToken::new(),
        );
        assert_eq!(h.started.recv().await.unwrap(), "one");
        let second = spawn_calls(
            h.registry.clone(),
            h.ctx.clone(),
            vec![call("two", 0)],
            CancellationToken::new(),
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
            h.registry.clone(),
            h.ctx.clone(),
            vec![call("running", 0), call("waiting", 1)],
            cancel.clone(),
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
