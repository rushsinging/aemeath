use crate::application::agent::{Agent, ToolCall, ToolExecution};
use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use hook::api::{HookData, ToolHookData};
use share::config::hooks::HookEvent;
use std::path::Path;
use std::sync::Arc;
use tools::ToolOutcome;

use super::tools::{
    emit_json_hook_context, log_tool_result, run_post_tool_hooks, send_tool_call_status,
    send_tool_result,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_non_agent<S>(
    context: &RuntimeTurnContext,
    agent: &Agent<'_>,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    non_agent_calls: &[ToolCall],
    language: &str,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let other_calls: Vec<&ToolCall> = non_agent_calls
        .iter()
        .filter(|c| c.name != "AskUserQuestion")
        .collect();

    if other_calls.is_empty() {
        return Vec::new();
    }

    if other_calls.len() == 1 {
        if agent.ctx.cancel.is_cancelled() {
            return vec![cancelled_result(other_calls[0], language)];
        }
        return execute_one_non_agent(
            context,
            agent,
            sink,
            hook_ui,
            hook_runner,
            other_calls[0],
            language,
            workspace_root,
        )
        .await;
    }

    execute_multiple_non_agent(
        context,
        agent,
        sink,
        hook_ui,
        hook_runner,
        &other_calls,
        language,
        workspace_root,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_multiple_non_agent<S>(
    context: &RuntimeTurnContext,
    agent: &Agent<'_>,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    other_calls: &[&ToolCall],
    language: &str,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let total_len = other_calls.len();
    let mut results: Vec<Option<ToolExecution>> = vec![None; total_len];
    let (concurrent_positions, sequential_positions) = partition_calls(agent, other_calls);

    if !concurrent_positions.is_empty() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(agent.ctx.max_tool_concurrency));
        let futures: Vec<_> = concurrent_positions
            .iter()
            .map(|&pos| {
                let call = other_calls[pos];
                let sink = sink.clone();
                let hook_ui = hook_ui.clone();
                let hook_runner = hook_runner.clone();
                let sem = semaphore.clone();
                let context = context.clone();
                let workspace_root = workspace_root.to_path_buf();
                async move {
                    if agent.ctx.cancel.is_cancelled() {
                        return (pos, Vec::new());
                    }
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    let result = execute_one_non_agent(
                        &context,
                        agent,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        call,
                        language,
                        &workspace_root,
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
        let result_vec = if agent.ctx.cancel.is_cancelled() {
            Vec::new()
        } else {
            execute_one_non_agent(
                context,
                agent,
                sink,
                hook_ui,
                hook_runner,
                call,
                language,
                workspace_root,
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

fn partition_calls(agent: &Agent<'_>, calls: &[&ToolCall]) -> (Vec<usize>, Vec<usize>) {
    let mut concurrent_positions = Vec::new();
    let mut sequential_positions = Vec::new();
    for (i, call) in calls.iter().enumerate() {
        let is_safe = agent
            .registry
            .get(&call.name)
            .map(|t| t.is_concurrency_safe())
            .unwrap_or(false);
        if is_safe {
            concurrent_positions.push(i);
        } else {
            sequential_positions.push(i);
        }
    }
    (concurrent_positions, sequential_positions)
}

fn cancelled_result(call: &ToolCall, language: &str) -> ToolExecution {
    let msg = match language {
        "zh" => "用户已取消",
        _ => "Cancelled by user",
    };
    ToolExecution::new(call, ToolOutcome::error(msg))
}

#[allow(clippy::too_many_arguments)]
async fn execute_one_non_agent<S>(
    context: &RuntimeTurnContext,
    agent: &Agent<'_>,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    call: &ToolCall,
    language: &str,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let _ = hook_ui
        .run_plain(
            hook_runner,
            HookEvent::PermissionRequest,
            Some(&call.name),
            HookData::Permission(hook::api::PermissionHookData {
                tool_name: call.name.clone(),
                permission_rule: "auto".to_string(),
            }),
            workspace_root,
        )
        .await;
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
    let pre_results = hook_ui
        .run_plain(
            hook_runner,
            HookEvent::PreToolUse,
            Some(&owned_call.name),
            HookData::Tool(ToolHookData {
                tool_name: owned_call.name.clone(),
                tool_input: owned_call.input.clone(),
                tool_output: None,
                is_error: None,
            }),
            workspace_root,
        )
        .await;
    if let Some(blocked_result) = pre_results.iter().find(|r| r.blocked) {
        log::debug!(target: crate::LOG_TARGET,
            "pretooluse timing blocked: kind=non_agent tool_name={} runtime_id={} provider_id={} exit_code={:?} error_present={}",
            owned_call.name,
            owned_call.id,
            owned_call.provider_id,
            blocked_result.exit_code,
            blocked_result.error.as_ref().is_some_and(|value| !value.is_empty()),
        );
        let default_blocked = match language {
            "zh" => "被 PreToolUse hook 阻止",
            _ => "Blocked by PreToolUse hook",
        };
        let error_detail = blocked_result.error.as_deref().unwrap_or(default_blocked);
        let result = ToolExecution::new(&owned_call, ToolOutcome::error(error_detail));
        send_tool_result(sink, context, &result).await;
        return vec![result];
    }
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing approved: kind=non_agent tool_name={} runtime_id={} provider_id={} hook_count={}",
        owned_call.name,
        owned_call.id,
        owned_call.provider_id,
        pre_results.len(),
    );
    send_tool_call_status(sink, context, &owned_call, RuntimeToolCallStatus::Ready).await;
    send_tool_call_status(sink, context, &owned_call, RuntimeToolCallStatus::Running).await;
    log::debug!(target: crate::LOG_TARGET,
        "tool execution timing running_sent: kind=non_agent tool_name={} runtime_id={} provider_id={}",
        owned_call.name,
        owned_call.id,
        owned_call.provider_id,
    );
    // Only Bash supports stdout streaming via progress_tx. For other tools,
    // skip the channel setup to avoid unnecessary overhead.
    let is_bash = owned_call.name == "Bash";

    let exec_results = if is_bash {
        // Set up progress channel for stdout streaming (mirrors agent_calls.rs
        // pattern).
        let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<tools::AgentProgressEvent>(32);
        let mut streaming_ctx = agent.ctx.clone();
        streaming_ctx.progress_tx = Some(prog_tx);
        let call_id = owned_call.id.clone();
        let stream_sink = sink.clone();
        let stream_context = context.clone();
        let forward_handle = tokio::spawn(async move {
            while let Some(event) = prog_rx.recv().await {
                let _ = stream_sink
                    .send_event(RuntimeStreamEvent::AgentProgress {
                        context: stream_context.clone(),
                        tool_id: call_id.clone(),
                        event,
                    })
                    .await;
            }
        });

        let results = vec![
            agent
                .execute_one_with_ctx(&owned_call, &streaming_ctx)
                .await,
        ];

        // Drop the sender so the forwarding task can complete naturally.
        streaming_ctx.progress_tx = None;

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
        vec![agent.execute_one_with_ctx(&owned_call, &agent.ctx).await]
    };

    let workspace = agent.ctx.workspace.persist().snapshot();
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
        log_tool_result(&ex.call_id, &owned_call.name, is_error, &ex.outcome.text);
        run_post_tool_hooks(
            sink,
            hook_ui,
            hook_runner,
            &owned_call,
            &ex,
            workspace_root,
            &agent.ctx,
        )
        .await;
        run_task_hooks(
            sink,
            hook_ui,
            hook_runner,
            &owned_call,
            &ex.outcome.text,
            is_error,
            workspace_root,
        )
        .await;
        // TasksSnapshot 由 loop_runner 在 PostToolExecutionSync 之后统一推送（#642），
        // 不再在此处发 TasksChanged 通知。
        send_tool_result(sink, context, &ex).await;
        out.push(ex);
    }
    out
}

async fn run_task_hooks<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    call: &ToolCall,
    output: &str,
    is_error: bool,
    workspace_root: &Path,
) where
    S: ChatEventSink,
{
    if !is_error && call.name == "TaskCreate" {
        emit_json_hook_context(
            sink,
            hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::TaskCreated,
                    None,
                    HookData::Tool(ToolHookData {
                        tool_name: "TaskCreate".to_string(),
                        tool_input: call.input.clone(),
                        tool_output: Some(output.to_string()),
                        is_error: Some(false),
                    }),
                    workspace_root,
                )
                .await,
        )
        .await;
    }
    if !is_error && call.name == "TaskUpdate" && output.contains("Status: Completed") {
        emit_json_hook_context(
            sink,
            hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::TaskCompleted,
                    None,
                    HookData::Tool(ToolHookData {
                        tool_name: "TaskUpdate".to_string(),
                        tool_input: call.input.clone(),
                        tool_output: Some(output.to_string()),
                        is_error: Some(false),
                    }),
                    workspace_root,
                )
                .await,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashSet;
    use std::sync::Arc;
    use tools::{ToolExecutionContext, ToolRegistry, TypedTool, TypedToolResult};

    struct ConcurrencyFlagTool {
        name: &'static str,
        safe: bool,
    }

    #[async_trait]
    impl TypedTool for ConcurrencyFlagTool {
        type Output = Value;

        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "concurrency classification test tool"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }

        fn is_concurrency_safe(&self) -> bool {
            self.safe
        }

        async fn call(
            &self,
            _input: Value,
            _ctx: &ToolExecutionContext,
        ) -> TypedToolResult<Self::Output> {
            TypedToolResult::success("ok", Value::Null)
        }
    }

    fn test_ctx() -> ToolExecutionContext {
        let cwd = std::env::current_dir().unwrap();
        ToolExecutionContext {
            resources: tools::ToolResources {
                agent_runner: None,
                registry: None,
                memory_config: share::config::MemoryConfig::default(),
                lang: "en".to_string(),
                allow_all: true,
            },
            workspace: project::wire_production_workspace(cwd)
                .expect("workspace 初始化成功")
                .into_views(),
            run_id: sdk::RunId::new_v7().to_string(),
            cancel: tokio_util::sync::CancellationToken::new(),
            read_files: Arc::new(std::sync::Mutex::new(HashSet::new())),
            session_reminders: None,
            plan_mode: None,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        }
    }

    fn call(name: &str, index: usize) -> ToolCall {
        ToolCall {
            provider_id: "provider-test".to_string(),
            id: sdk::ids::ToolCallId::from_legacy_or_new(&format!("call-{index}")),
            name: name.to_string(),
            index,
            input: serde_json::json!({}),
        }
    }

    #[test]
    fn test_partition_calls_routes_concurrency_safe_tools_to_concurrent() {
        let registry = ToolRegistry::new();
        registry.register(ConcurrencyFlagTool {
            name: "safe_a",
            safe: true,
        });
        registry.register(ConcurrencyFlagTool {
            name: "safe_b",
            safe: true,
        });
        let agent = Agent {
            registry: &registry,
            ctx: test_ctx(),
        };
        let calls = [call("safe_a", 0), call("safe_b", 1)];
        let refs = calls.iter().collect::<Vec<_>>();

        let (concurrent, sequential) = partition_calls(&agent, &refs);

        assert_eq!(concurrent, vec![0, 1]);
        assert!(sequential.is_empty());
    }

    #[test]
    fn test_partition_calls_routes_non_concurrency_safe_tools_to_sequential() {
        let registry = ToolRegistry::new();
        registry.register(ConcurrencyFlagTool {
            name: "unsafe_a",
            safe: false,
        });
        registry.register(ConcurrencyFlagTool {
            name: "unsafe_b",
            safe: false,
        });
        let agent = Agent {
            registry: &registry,
            ctx: test_ctx(),
        };
        let calls = [call("unsafe_a", 0), call("unsafe_b", 1)];
        let refs = calls.iter().collect::<Vec<_>>();

        let (concurrent, sequential) = partition_calls(&agent, &refs);

        assert!(concurrent.is_empty());
        assert_eq!(sequential, vec![0, 1]);
    }

    #[test]
    fn test_partition_calls_preserves_mixed_positions() {
        let registry = ToolRegistry::new();
        registry.register(ConcurrencyFlagTool {
            name: "safe",
            safe: true,
        });
        registry.register(ConcurrencyFlagTool {
            name: "unsafe",
            safe: false,
        });
        let agent = Agent {
            registry: &registry,
            ctx: test_ctx(),
        };
        let calls = [call("safe", 0), call("unsafe", 1), call("safe", 2)];
        let refs = calls.iter().collect::<Vec<_>>();

        let (concurrent, sequential) = partition_calls(&agent, &refs);

        assert_eq!(concurrent, vec![0, 2]);
        assert_eq!(sequential, vec![1]);
    }

    #[test]
    fn test_partition_calls_routes_unknown_tools_to_sequential() {
        let registry = ToolRegistry::new();
        let agent = Agent {
            registry: &registry,
            ctx: test_ctx(),
        };
        let calls = [call("missing", 0)];
        let refs = calls.iter().collect::<Vec<_>>();

        let (concurrent, sequential) = partition_calls(&agent, &refs);

        assert!(concurrent.is_empty());
        assert_eq!(sequential, vec![0]);
    }
}
