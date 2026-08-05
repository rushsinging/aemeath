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
use hook::{HookInvocation, HookPort, PermissionInput, PostToolUseFailureInput, PostToolUseInput};

use sdk::ids::ToolCallId;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::{ToolOutcome, ToolSuspension};

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
    policy: &dyn policy::PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    agent: &Agent,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    cancel: &CancellationToken,
    language: &str,
    workspace_root: &std::path::Path,
    guarded_calls: &[(ToolCall, crate::application::loop_engine::ToolGuardDecision)],
) -> ToolRoundResult
where
    S: ChatEventSink,
{
    let prepared = prepare_tool_round(
        guarded_calls,
        catalog,
        policy,
        run_id,
        step_id,
        workspace_root,
    );
    let denied_results = deny_tool_calls(
        &prepared.denied,
        sink,
        context,
        hook_port,
        activities,
        step_id,
        cancel,
        workspace_root,
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
            tools::ToolExecutionOutcome::Suspended(suspension) => {
                let questions = match suspension {
                    ToolSuspension::UserInteraction(spec) => spec
                        .questions
                        .iter()
                        .map(|q| SuspendedQuestion {
                            prompt: q.prompt.clone(),
                            options: q.options.iter().map(|o| o.title.clone()).collect(),
                            allow_multi: q.allow_multi,
                        })
                        .collect(),
                };
                suspensions.push(SuspendedToolCall {
                    call: (*call).clone(),
                    questions,
                });
            }
            outcome => ask_user_terminal.push(ToolExecution::new(
                call,
                crate::application::tool::agent::legacy_outcome(outcome),
            )),
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
        workspace_root,
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
        workspace_root,
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
    // #1248 Task 5: Map RequireApproval calls from policy to engine-level ApprovalRequiredCall.
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
    hook_port: &Arc<dyn HookPort>,
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
            HookInvocation::PermissionDenied(PermissionInput {
                tool_name: call.call.name.clone(),
                permission_rule: "deny".to_string(),
            }),
            workspace_root,
            cancel,
        )
        .await;
        // 发送 ToolCall 事件，让 pending 占位行获取 LLM 的 tool_use_id，
        // 后续 ToolResult 中的 mark_tool_header_done 才能精确匹配（Bug #52）。
        let call_id = call.call.id.clone();
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCallUpdate {
                context: context.clone(),
                id: call_id.clone(),
                provider_id: Some(call.call.provider_id.clone()),
                name: call.call.name.clone(),
                index: call.call.index,
                arguments_delta: None,
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
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    step_id: &sdk::RunStepId,
    call: &ToolCall,
    execution: &ToolExecution,
    cancel: &CancellationToken,
    workspace_root: &std::path::Path,
) {
    let output = &execution.outcome.text;
    let is_error = execution.outcome.is_error;

    let _ = dispatch_hook(
        hook_port,
        activities,
        step_id,
        HookInvocation::PostToolUse(PostToolUseInput {
            tool_name: call.name.clone(),
            tool_input: call.input.clone(),
            tool_output: output.to_string(),
            is_error,
        }),
        workspace_root,
        cancel,
    )
    .await;

    if is_error {
        let _ = dispatch_hook(
            hook_port,
            activities,
            step_id,
            HookInvocation::PostToolUseFailure(PostToolUseFailureInput {
                tool_name: call.name.clone(),
                tool_input: call.input.clone(),
                error: output.to_string(),
            }),
            workspace_root,
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
        .send_event(RuntimeStreamEvent::ToolCallUpdate {
            context: context.clone(),
            id: call.id.clone(),
            provider_id: Some(call.provider_id.clone()),
            name: call.name.clone(),
            index: call.index,
            arguments_delta: None,
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
mod tests {
    use super::{execute_tool_round, send_tool_result};
    use crate::application::loop_engine::chat::{
        ChatEventSink, EventFuture, RuntimeRunContext, RuntimeStreamEvent,
    };
    use crate::application::loop_engine::ToolGuardDecision;
    use crate::application::tool::agent::{Agent, ToolCall, ToolExecution};
    use crate::application::tool::coordination::complete_cancelled_tool_round;
    use async_trait::async_trait;
    use hook::{HookInvocation, HookOutcome, HookPort};
    use sdk::ids::{ChatId, ChatRunId, ToolCallId};
    use serde_json::Value;
    use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
    use share::message::ContentBlock;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tools::ToolOutcome;
    use tools::{ToolExecutionContext, TypedTool, TypedToolResult};

    /// A test HookPort that always returns Continue.
    struct NoOpHookPort;

    #[async_trait]
    impl HookPort for NoOpHookPort {
        async fn dispatch(
            &self,
            _invocation: HookInvocation,
            _cancellation: &dyn hook::CancellationSignal,
        ) -> HookOutcome {
            HookOutcome::proceed()
        }
    }

    fn noop_hook_port() -> Arc<dyn HookPort> {
        Arc::new(NoOpHookPort)
    }

    #[derive(Clone, Default)]
    struct RecordingSink {
        events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
    }

    impl RecordingSink {
        fn lifecycle_events(&self) -> Vec<(String, String)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|event| match event {
                    RuntimeStreamEvent::ToolCallUpdate { id, status, .. } => {
                        Some((id.to_string(), format!("{status:?}")))
                    }
                    RuntimeStreamEvent::ToolResult { id, .. } => {
                        Some((id.to_string(), "Result".to_string()))
                    }
                    _ => None,
                })
                .collect()
        }
    }

    impl ChatEventSink for RecordingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                self.events.lock().unwrap().push(event);
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    struct UnsafeLifecycleTool;

    struct BlockingAgentTool {
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl TypedTool for BlockingAgentTool {
        type Output = Value;

        fn name(&self) -> &str {
            "Agent"
        }

        fn description(&self) -> &str {
            "blocking Agent cancellation test"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({"type":"object"})
        }

        fn cancellation(&self) -> tools::CancellationDeclaration {
            tools::CancellationDeclaration::Cooperative
        }

        async fn call(
            &self,
            _input: Value,
            ctx: &ToolExecutionContext,
        ) -> TypedToolResult<Self::Output> {
            self.started.notify_one();
            ctx.cancellation().cancelled().await;
            TypedToolResult::error("Agent cancelled by current Step")
        }
    }

    #[async_trait]
    impl TypedTool for UnsafeLifecycleTool {
        type Output = Value;

        fn name(&self) -> &str {
            "UnsafeLifecycle"
        }

        fn description(&self) -> &str {
            "non-concurrency-safe lifecycle test tool"
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({"type":"object"})
        }

        fn is_concurrency_safe(&self) -> bool {
            false
        }

        async fn call(
            &self,
            input: Value,
            _ctx: &ToolExecutionContext,
        ) -> TypedToolResult<Self::Output> {
            TypedToolResult::success(
                input.get("label").and_then(Value::as_str).unwrap_or("ok"),
                Value::Null,
            )
        }
    }

    fn test_tool_context() -> ToolExecutionContext {
        crate::application::run::workspace_test_support::test_tool_execution_context(
            std::env::current_dir().unwrap(),
            tokio_util::sync::CancellationToken::new(),
        )
    }

    fn lifecycle_call(index: usize) -> ToolCall {
        ToolCall {
            id: ToolCallId::from_legacy_or_new(&format!("call-{index}")),
            provider_id: format!("provider-{index}"),
            name: "UnsafeLifecycle".to_string(),
            index,
            input: serde_json::json!({"label": format!("call-{index}")}),
        }
    }

    #[tokio::test]
    async fn tool_round_step_cancellation_reaches_running_agent_context() {
        let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
        let started = Arc::new(tokio::sync::Notify::new());
        registry.register(BlockingAgentTool {
            started: started.clone(),
        });
        let ctx = test_tool_context();
        let agent = Arc::new(Agent::for_test(registry.as_ref(), ctx, 10));
        let sink = RecordingSink::default();
        let hook_port = noop_hook_port();
        let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let call = ToolCall {
            id: ToolCallId::from_legacy_or_new("agent-cancel"),
            provider_id: "provider-agent-cancel".to_string(),
            name: "Agent".to_string(),
            index: 0,
            input: serde_json::json!({}),
        };
        let step_cancel = tokio_util::sync::CancellationToken::new();
        let execution_agent = agent.clone();
        let execution_context = context.clone();
        let execution_sink = sink.clone();
        let execution_hook_port = hook_port.clone();
        let execution_workspace_root = workspace_root.clone();
        let execution_call = call.clone();
        let execution_cancel = step_cancel.clone();
        let execution_activities = crate::application::activity::ActivityCoordinator::new(
            sdk::RunId::new_v7(),
            Arc::new(crate::application::activity::SystemActivityClock),
            Arc::new(crate::application::activity::UuidV7ActivityIdSource),
        );
        let handle = tokio::spawn(async move {
            execute_tool_round(
                &execution_context,
                std::slice::from_ref(&execution_call),
                &execution_agent.catalog,
                &policy::AllowAllPolicy,
                &sdk::RunId::new_v7(),
                &sdk::RunStepId::new_v7(),
                execution_agent.as_ref(),
                &execution_sink,
                &execution_hook_port,
                &execution_activities,
                &execution_cancel,
                "en",
                &execution_workspace_root,
                &[(execution_call.clone(), ToolGuardDecision::Allow)],
            )
            .await
        });
        started.notified().await;

        step_cancel.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("当前 Step 取消必须到达运行中的 Agent execution context")
            .unwrap();

        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].outcome.is_error);
        assert!(result.results[0].outcome.text.contains("cancel"));
    }

    #[tokio::test]
    async fn allow_all_bypasses_soft_block_and_blocking_pre_tool_hook() {
        let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
        registry.register(UnsafeLifecycleTool);
        let ctx = test_tool_context();
        let agent = Agent::for_test(registry.as_ref(), ctx, 10);
        let sink = RecordingSink::default();
        let hook_port = noop_hook_port();
        let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let call = lifecycle_call(0);
        let activities = crate::application::activity::ActivityCoordinator::new(
            sdk::RunId::new_v7(),
            Arc::new(crate::application::activity::SystemActivityClock),
            Arc::new(crate::application::activity::UuidV7ActivityIdSource),
        );

        let result = execute_tool_round(
            &context,
            std::slice::from_ref(&call),
            &agent.catalog,
            &policy::AllowAllPolicy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            &agent,
            &sink,
            &hook_port,
            &activities,
            &tokio_util::sync::CancellationToken::new(),
            "en",
            &workspace_root,
            &[(
                call.clone(),
                ToolGuardDecision::SoftBlock {
                    reason: "loop".to_string(),
                },
            )],
        )
        .await;

        assert_eq!(result.fuse_bypassed, vec![call.id]);
        assert_eq!(result.results.len(), 1);
        assert!(
            !result.results[0].outcome.is_error,
            "AllowAll must execute the tool"
        );
    }

    /// #1515: PreToolUse 事件 hook 必须无条件执行——AllowAll 只放行授权性
    /// 限制，不得跳过事件 hook。修复前 PreToolUse 被错误门控跳过、工具正常
    /// 执行；修复后 hook exit 2 阻断工具。
    #[tokio::test]
    async fn allow_all_still_runs_blocking_pre_tool_hook() {
        let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
        registry.register(UnsafeLifecycleTool);
        let ctx = test_tool_context();
        let agent = Agent::for_test(registry.as_ref(), ctx, 10);
        let sink = RecordingSink::default();
        let mut events = HashMap::new();
        events.insert(
            HookEvent::PreToolUse,
            vec![HookEntry {
                matcher: String::new(),
                command: "exit 2".to_string(),
                timeout: 5,
            }],
        );
        let hook_port: Arc<dyn HookPort> = Arc::new(
            hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(
                share::config::Config {
                    hooks: HooksConfig {
                        events,
                        ..HooksConfig::default()
                    },
                    ..share::config::Config::default()
                },
            ))
            .unwrap(),
        );
        let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let call = lifecycle_call(0);
        let activities = crate::application::activity::ActivityCoordinator::new(
            sdk::RunId::new_v7(),
            Arc::new(crate::application::activity::SystemActivityClock),
            Arc::new(crate::application::activity::UuidV7ActivityIdSource),
        );

        let result = execute_tool_round(
            &context,
            std::slice::from_ref(&call),
            &agent.catalog,
            &policy::AllowAllPolicy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            &agent,
            &sink,
            &hook_port,
            &activities,
            &tokio_util::sync::CancellationToken::new(),
            "en",
            &workspace_root,
            &[(call.clone(), ToolGuardDecision::Allow)],
        )
        .await;

        assert_eq!(result.results.len(), 1);
        assert!(
            result.results[0].outcome.is_error,
            "AllowAll 不得跳过 PreToolUse 事件 hook：exit 2 应阻断工具"
        );
        assert!(
            result.results[0]
                .outcome
                .text
                .contains("Blocked by PreToolUse hook"),
            "阻断消息应来自 PreToolUse hook，实际 = {:?}",
            result.results[0].outcome.text
        );
    }

    #[tokio::test]
    async fn test_non_concurrency_safe_tools_emit_running_after_previous_result() {
        let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
        registry.register(UnsafeLifecycleTool);
        let ctx = test_tool_context();
        let agent = Agent::for_test(registry.as_ref(), ctx, 10);
        let sink = RecordingSink::default();
        let hook_port = noop_hook_port();
        let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let activities = crate::application::activity::ActivityCoordinator::new(
            sdk::RunId::new_v7(),
            Arc::new(crate::application::activity::SystemActivityClock),
            Arc::new(crate::application::activity::UuidV7ActivityIdSource),
        );
        let calls = vec![lifecycle_call(0), lifecycle_call(1)];
        let guarded_calls = calls
            .iter()
            .cloned()
            .map(|call| (call, ToolGuardDecision::Allow))
            .collect::<Vec<_>>();

        let _ = execute_tool_round(
            &context,
            &calls,
            &agent.catalog,
            &policy::AllowAllPolicy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            &agent,
            &sink,
            &hook_port,
            &activities,
            &tokio_util::sync::CancellationToken::new(),
            "en",
            &workspace_root,
            &guarded_calls,
        )
        .await;

        let lifecycle = sink.lifecycle_events();

        assert_eq!(
            lifecycle,
            vec![
                (calls[0].id.to_string(), "Ready".to_string()),
                (calls[0].id.to_string(), "Running".to_string()),
                (calls[0].id.to_string(), "Result".to_string()),
                (calls[1].id.to_string(), "Ready".to_string()),
                (calls[1].id.to_string(), "Running".to_string()),
                (calls[1].id.to_string(), "Result".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn oversized_tool_result_event_uses_materialized_projection() {
        const THRESHOLD: usize = 50_000;
        let oversized = "界".repeat(THRESHOLD + 1);
        assert!(oversized.chars().count() > THRESHOLD);
        let execution = ToolExecution::from_parts(
            ToolCallId::new_v7(),
            "provider-oversized".to_string(),
            "UnknownTool".to_string(),
            ToolOutcome::new(
                oversized.clone(),
                serde_json::json!({ "unexpected": oversized }),
                Vec::new(),
            ),
        );
        let sink = RecordingSink::default();
        let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
        let materializer = crate::application::tool::test_support::test_tool_result_materializer();

        send_tool_result(
            &sink,
            &context,
            &execution,
            materializer.as_ref(),
            "event-session",
        )
        .await;

        let events = sink.events.lock().unwrap();
        let [RuntimeStreamEvent::ToolResult {
            output, content, ..
        }] = events.as_slice()
        else {
            panic!("expected one tool result event");
        };
        assert!(!output.contains(&oversized));
        assert_eq!(
            content.get("text").and_then(Value::as_str),
            Some(output.as_str())
        );
        assert_eq!(
            content.pointer("/blob/status").and_then(Value::as_str),
            Some("persisted")
        );
        assert_eq!(
            content.pointer("/blob/locator").and_then(Value::as_str),
            Some("tool-result://event-session/provider-oversized")
        );
        assert!(!content.to_string().contains(&oversized));
    }

    #[tokio::test]
    async fn cancelled_tool_round_materializes_one_result_for_each_provider_call() {
        let calls = vec![lifecycle_call(0), lifecycle_call(1)];
        let completed = ToolExecution::new(
            &calls[0],
            ToolOutcome::new("finished", Value::Null, Vec::new()),
        );
        let results = complete_cancelled_tool_round(&calls, vec![completed]).results;
        let materializer = crate::application::tool::test_support::test_tool_result_materializer();

        let message = crate::application::loop_engine::shared::materialize_tool_results(
            materializer.as_ref(),
            results,
            "test-cancelled-round",
        )
        .await;

        assert_eq!(message.content.len(), 2);
        let provider_ids = message
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::ToolResult { tool_use_id, .. } => tool_use_id.as_str(),
                other => panic!("expected tool result, got {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(provider_ids, ["provider-0", "provider-1"]);
    }

    #[tokio::test]
    async fn test_materialized_tool_results_use_provider_id_not_runtime_id() {
        let results = vec![ToolExecution::from_parts(
            ToolCallId::new_v7(),
            "provider-id".to_string(),
            "Bash".to_string(),
            ToolOutcome::new("ok", serde_json::json!({ "text": "ok" }), Vec::new()),
        )];
        let materializer = crate::application::tool::test_support::test_tool_result_materializer();
        let message = crate::application::loop_engine::shared::materialize_tool_results(
            materializer.as_ref(),
            results,
            "test-provider-id",
        )
        .await;

        let [ContentBlock::ToolResult { tool_use_id, .. }] = message.content.as_slice() else {
            panic!("expected one tool result");
        };
        assert_eq!(tool_use_id, "provider-id");
    }

    #[tokio::test]
    async fn test_materialized_tool_results_persist_oversized_tui_result() {
        const THRESHOLD: usize = 50_000;
        let session_id = format!("test-tui-{}", std::process::id());
        let oversized = "x".repeat(THRESHOLD + 1);
        let results = vec![ToolExecution::from_parts(
            ToolCallId::new_v7(),
            "provider-oversized".to_string(),
            "Bash".to_string(),
            ToolOutcome::new(
                oversized,
                serde_json::json!({ "text": "oversized" }),
                Vec::new(),
            ),
        )];
        let materializer = crate::application::tool::test_support::test_tool_result_materializer();
        let message = crate::application::loop_engine::shared::materialize_tool_results(
            materializer.as_ref(),
            results,
            &session_id,
        )
        .await;

        let [ContentBlock::ToolResult { content, .. }] = message.content.as_slice() else {
            panic!("expected one tool result");
        };
        let content = match content {
            serde_json::Value::Object(map) => map,
            other => panic!("tool result should be json object, got {other:?}"),
        };
        let text = content
            .get("text")
            .and_then(|value| value.as_str())
            .expect("persisted reference should be in text field");
        assert!(text.contains("<persisted-output>"));
        assert!(text.len() < THRESHOLD);
        assert!(text.contains(&session_id));
    }

    // #1248 Task 5: Bridge resolve tests moved to engine-level tests
    // (interaction_routing module in loop_engine/tests.rs).
    // resolve_ask_user_via_bridge is deleted — the engine handles
    // all interaction routing via InteractionCoordinator.
}
