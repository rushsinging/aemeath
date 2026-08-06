use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingHandler {
    accepted_tool: &'static str,
    observations: Arc<AtomicUsize>,
}

#[async_trait]
impl CommittedSideEffectHandler for CountingHandler {
    fn accepts(&self, execution: &ToolExecution) -> bool {
        execution.tool_name == self.accepted_tool
    }

    async fn observe(
        &self,
        _call: &ToolCall,
        _execution: &ToolExecution,
        _step_id: &sdk::RunStepId,
        _cancel: &tokio_util::sync::CancellationToken,
    ) {
        self.observations.fetch_add(1, Ordering::SeqCst);
    }
}

fn call(name: &str) -> ToolCall {
    ToolCall {
        id: sdk::ToolCallId::new_v7(),
        provider_id: "provider-call".to_string(),
        name: name.to_string(),
        index: 0,
        input: serde_json::json!({}),
    }
}

#[tokio::test]
async fn dispatcher_routes_only_to_accepting_capability_handlers() {
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let dispatcher = CommittedSideEffectDispatcher::new(vec![
        Arc::new(CountingHandler {
            accepted_tool: "TaskCreate",
            observations: first.clone(),
        }),
        Arc::new(CountingHandler {
            accepted_tool: "WorkspaceCommit",
            observations: second.clone(),
        }),
    ]);
    let task_call = call("TaskCreate");
    let task_execution = ToolExecution::new(
        &task_call,
        tools::ToolOutcome::new("ok", serde_json::Value::Null, Vec::new()),
    );

    dispatcher
        .observe(
            &task_call,
            &task_execution,
            &sdk::RunStepId::new_v7(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;

    assert_eq!(first.load(Ordering::SeqCst), 1);
    assert_eq!(second.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn dispatcher_supports_a_second_independent_capability() {
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let dispatcher = CommittedSideEffectDispatcher::new(vec![
        Arc::new(CountingHandler {
            accepted_tool: "TaskCreate",
            observations: first.clone(),
        }),
        Arc::new(CountingHandler {
            accepted_tool: "WorkspaceCommit",
            observations: second.clone(),
        }),
    ]);
    let workspace_call = call("WorkspaceCommit");
    let workspace_execution = ToolExecution::new(
        &workspace_call,
        tools::ToolOutcome::new("ok", serde_json::Value::Null, Vec::new()),
    );

    dispatcher
        .observe(
            &workspace_call,
            &workspace_execution,
            &sdk::RunStepId::new_v7(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;

    assert_eq!(first.load(Ordering::SeqCst), 0);
    assert_eq!(second.load(Ordering::SeqCst), 1);
}
