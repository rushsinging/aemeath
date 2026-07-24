use crate::application::interaction::InteractionBridge;
use crate::application::main_loop::looping::agent_calls::execute_agent_calls;
#[allow(unused_imports)]
use crate::application::main_loop::looping::ask_user::ask_user;
use crate::application::main_loop::looping::hook_ui::dispatch_hook;
use crate::application::main_loop::looping::non_agent::execute_non_agent;
use crate::application::main_loop::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use crate::application::subagent::{Agent, ToolCall, ToolExecution};
use crate::application::tool_coordination::{prepare_tool_round, restore_tool_call_order};
use hook::{HookInvocation, HookPort, PermissionInput, PostToolUseFailureInput, PostToolUseInput};

use sdk::ids::ToolCallId;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::{ToolCatalogPort, ToolExecutionPort};
use tools::{ToolOutcome, ToolSuspension};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tool_round<S>(
    context: &RuntimeTurnContext,
    tool_calls: &[ToolCall],
    tool_catalog: &Arc<dyn ToolCatalogPort>,
    tool_execution: &Arc<dyn ToolExecutionPort>,
    policy: &dyn policy::PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    agent: &Agent,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    cancel: &CancellationToken,
    language: &str,
    workspace_root: &std::path::Path,
    guarded_calls: &[(ToolCall, crate::application::loop_engine::ToolGuardDecision)],
    interaction_bridge: &Arc<InteractionBridge>,
) -> (Vec<ToolExecution>, Vec<ToolCallId>)
where
    S: ChatEventSink,
{
    let catalog = match tool_catalog.snapshot(
        &tools::RegistryScopeName::new("main"),
        &tools::ToolProfileName::new("main-full"),
    ) {
        Ok(catalog) => catalog,
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "tool catalog snapshot failed: {error}");
            return (
                tool_calls
                    .iter()
                    .map(|call| {
                        ToolExecution::new(
                            call,
                            ToolOutcome::error(format!("tool catalog unavailable: {error}")),
                        )
                    })
                    .collect(),
                Vec::new(),
            );
        }
    };
    let prepared = prepare_tool_round(
        guarded_calls,
        &catalog,
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
        cancel,
        workspace_root,
    )
    .await;
    let fuse_bypassed = prepared.fuse_bypassed.clone();
    let approved = prepared.executable;
    let fused_results =
        publish_guard_blocked(prepared.guard_blocked, tool_calls, sink, context).await;

    let (agent_approved, non_agent_approved): (Vec<_>, Vec<_>) = approved
        .into_iter()
        .partition(|prepared| prepared.call.name == "Agent");

    // AskUser must cross the same execution port as every production tool.
    // Only a typed Suspended outcome enters Runtime's existing waiter; every
    // failure/cancellation remains a concrete ToolExecution result.
    let mut ask_user_suspensions = Vec::new();
    let mut ask_user_terminal = Vec::new();
    for prepared in non_agent_approved
        .iter()
        .filter(|prepared| prepared.call.name == "AskUserQuestion")
    {
        let call = &prepared.call;
        let mut input = call.input.clone();
        tools::strip_runtime_meta(&mut input);
        let invocation =
            tools::ToolInvocation::new(call.name.as_str(), input, agent.ctx.scope().clone())
                .with_authorization(prepared.authorization);
        match tool_execution
            .execute(invocation, agent.ctx.cancellation().as_ref())
            .await
        {
            tools::ToolExecutionOutcome::Suspended(suspension) => {
                ask_user_suspensions.push((call, suspension, prepared.authorization));
            }
            outcome => ask_user_terminal.push(ToolExecution::new(
                call,
                crate::application::subagent::legacy_outcome(outcome),
            )),
        }
    }
    let ask_user_results = resolve_ask_user_via_bridge(
        context,
        sink,
        hook_port,
        &ask_user_suspensions,
        cancel,
        workspace_root,
        run_id,
        interaction_bridge,
    )
    .await;
    let non_agent_results = execute_non_agent(
        context,
        agent,
        sink,
        hook_port,
        &non_agent_approved,
        language,
        workspace_root,
        policy,
        run_id,
        step_id,
    )
    .await;
    let agent_results = execute_agent_calls(
        context,
        &agent_approved,
        tool_execution,
        &agent.ctx,
        &agent.agent_semaphore,
        &agent.workspace_persist,
        sink,
        hook_port,
        cancel,
        workspace_root,
        &catalog,
        policy,
        run_id,
        step_id,
    )
    .await;

    let results = ask_user_results
        .into_iter()
        .chain(ask_user_terminal)
        .chain(non_agent_results)
        .chain(agent_results)
        .chain(fused_results)
        .chain(denied_results)
        .collect();
    (restore_tool_call_order(tool_calls, results), fuse_bypassed)
}

/// #1246: Resolve AskUserQuestion suspensions via InteractionBridge.
///
/// Each suspension is registered and resolved one at a time (in original
/// ToolCall order), ensuring Run has at most one PendingInteraction.
/// Reply maps to ToolSuccess; cancel maps to ToolCancelled.
async fn resolve_ask_user_via_bridge<S>(
    context: &RuntimeTurnContext,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    suspended_calls: &[(&ToolCall, ToolSuspension, tools::AuthorizationContext)],
    cancel: &CancellationToken,
    workspace_root: &std::path::Path,
    run_id: &sdk::RunId,
    interaction_bridge: &Arc<InteractionBridge>,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    if suspended_calls.is_empty() {
        return Vec::new();
    }

    // Permission request hooks (same as legacy ask_user).
    for (call, _, authorization) in suspended_calls {
        if !authorization.enforce_permission_hooks {
            continue;
        }
        let _ = dispatch_hook(
            hook_port,
            sink,
            HookInvocation::PermissionRequest(PermissionInput {
                tool_name: call.name.clone(),
                permission_rule: "manual".to_string(),
            }),
            workspace_root,
            cancel,
        )
        .await;
    }

    let mut results = Vec::new();
    for (call, suspension, _) in suspended_calls {
        let questions = match suspension {
            ToolSuspension::UserInteraction(spec) => spec
                .questions
                .iter()
                .map(|q| sdk::UserQuestion {
                    prompt: q.prompt.clone(),
                    options: q.options.iter().map(|o| o.title.clone()).collect(),
                    allow_multi: q.allow_multi,
                })
                .collect::<Vec<_>>(),
        };

        let request = sdk::InteractionRequest {
            id: sdk::InteractionRequestId::new_v7(),
            run_id: run_id.clone(),
            body: sdk::InteractionRequestBody::UserQuestions(questions),
        };

        let receiver = match interaction_bridge.register(request.clone()) {
            Ok(rx) => rx,
            Err(outcome) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "interaction register failed: {outcome:?}"
                );
                results.push(ToolExecution::new(
                    call,
                    ToolOutcome::error("Interaction already completed"),
                ));
                continue;
            }
        };

        sink.send_event(RuntimeStreamEvent::InteractionRequested { request })
            .await;

        let completion = tokio::select! {
            result = receiver => match result {
                Ok(completion) => completion,
                Err(_) => {
                    results.push(ToolExecution::new(
                        call,
                        ToolOutcome::error("Interaction waiter dropped"),
                    ));
                    continue;
                }
            },
            () = cancel.cancelled() => {
                results.push(ToolExecution::new(
                    call,
                    ToolOutcome::error("Cancelled by user"),
                ));
                continue;
            }
        };

        match completion {
            crate::application::interaction::InteractionCompletion::Replied(reply) => match reply {
                sdk::InteractionReply::UserQuestions(answers) => {
                    let answer_text = answers
                        .into_iter()
                        .map(|a| a.0)
                        .collect::<Vec<_>>()
                        .join("\n");
                    let outcome = ToolOutcome::new(
                        answer_text.clone(),
                        serde_json::json!({"answer": answer_text}),
                        Vec::new(),
                    );
                    let execution = ToolExecution::new(call, outcome);
                    crate::application::main_loop::looping::tools::send_tool_result(
                        sink, context, &execution,
                    )
                    .await;
                    results.push(execution);
                }
                _ => {
                    results.push(ToolExecution::new(
                        call,
                        ToolOutcome::error("Unexpected reply type"),
                    ));
                }
            },
            crate::application::interaction::InteractionCompletion::Cancelled(_) => {
                results.push(ToolExecution::new(
                    call,
                    ToolOutcome::error("Cancelled by user"),
                ));
            }
        }
    }
    results
}

async fn publish_guard_blocked<S>(
    blocked: Vec<ToolExecution>,
    calls: &[ToolCall],
    sink: &S,
    context: &RuntimeTurnContext,
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
        send_tool_result(sink, context, execution).await;
    }
    blocked
}

async fn deny_tool_calls<S>(
    denied: &[crate::application::tool_coordination::DeniedToolCall],
    sink: &S,
    context: &RuntimeTurnContext,
    hook_port: &Arc<dyn HookPort>,
    cancel: &CancellationToken,
    workspace_root: &std::path::Path,
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
            sink,
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
        };
        let execution = ToolExecution::from_parts(
            call_id,
            call.call.provider_id.clone(),
            call.call.name.clone(),
            outcome,
        );
        send_tool_result(sink, context, &execution).await;
        denied_results.push(execution);
    }
    denied_results
}

pub(crate) async fn run_post_tool_hooks<S>(
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    call: &ToolCall,
    execution: &ToolExecution,
    cancel: &CancellationToken,
    workspace_root: &std::path::Path,
) where
    S: ChatEventSink,
{
    let output = &execution.outcome.text;
    let is_error = execution.outcome.is_error;

    let _ = dispatch_hook(
        hook_port,
        sink,
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
            sink,
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
    context: &RuntimeTurnContext,
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
    context: &RuntimeTurnContext,
    execution: &ToolExecution,
) where
    S: ChatEventSink,
{
    let _ = sink
        .send_event(RuntimeStreamEvent::ToolResult {
            context: context.clone(),
            id: execution.call_id.clone(),
            provider_id: execution.provider_id.clone(),
            tool_name: execution.tool_name.clone(),
            output: execution.outcome.text.clone(),
            content: execution.outcome.data.clone(),
            is_error: execution.outcome.is_error,
            images: execution.outcome.images.clone(),
        })
        .await;
}

pub(crate) async fn tool_results_for_api(
    materializer: &crate::application::tool_result_materialization::ToolResultMaterializer,
    results: Vec<ToolExecution>,
    session_id: &str,
) -> share::message::Message {
    let error_count = results.iter().filter(|ex| ex.outcome.is_error).count();
    log::debug!(
        target: crate::LOG_TARGET,
        "tool_results_for_api: {} typed ToolExecution(s) → wire ({} error)",
        results.len(),
        error_count
    );
    crate::application::loop_engine::shared::materialize_tool_results(
        materializer,
        results,
        session_id,
    )
    .await
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
    use super::{execute_tool_round, tool_results_for_api};
    use crate::application::loop_engine::ToolGuardDecision;
    use crate::application::main_loop::looping::{
        ChatEventSink, EventFuture, RuntimeStreamEvent, RuntimeTurnContext,
    };
    use crate::application::subagent::{Agent, ToolCall, ToolExecution};
    use crate::application::tool_coordination::complete_cancelled_tool_round;
    use async_trait::async_trait;
    use hook::{HookInvocation, HookOutcome, HookPort};
    use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
    use serde_json::Value;
    use share::message::ContentBlock;
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;
    use tools::ToolOutcome;
    use tools::{ToolExecutionContext, TypedTool, TypedToolResult};

    /// A test HookPort that always returns Continue.
    struct NoOpHookPort;

    #[async_trait]
    impl HookPort for NoOpHookPort {
        async fn dispatch(
            &self,
            _invocation: HookInvocation,
            _cancellation: &CancellationToken,
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
        crate::application::testing::test_tool_execution_context(
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
    async fn allow_all_bypasses_soft_block_and_blocking_pre_tool_hook() {
        let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
        registry.register(UnsafeLifecycleTool);
        let ctx = test_tool_context();
        let agent = Agent::for_test(registry.as_ref(), ctx, 10);
        let sink = RecordingSink::default();
        let hook_port = noop_hook_port();
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let call = lifecycle_call(0);
        let ports = registry.build(agent.ctx.clone());

        let (results, bypassed) = execute_tool_round(
            &context,
            std::slice::from_ref(&call),
            &ports.catalog_port(),
            &ports.execution(),
            &policy::AllowAllPolicy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            &agent,
            &sink,
            &hook_port,
            &tokio_util::sync::CancellationToken::new(),
            "en",
            &workspace_root,
            &[(
                call.clone(),
                ToolGuardDecision::SoftBlock {
                    reason: "loop".to_string(),
                },
            )],
            &std::sync::Arc::new(crate::application::interaction::InteractionBridge::new()),
        )
        .await;

        assert_eq!(bypassed, vec![call.id]);
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].outcome.is_error,
            "AllowAll must execute the tool"
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
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let calls = vec![lifecycle_call(0), lifecycle_call(1)];
        let guarded_calls = calls
            .iter()
            .cloned()
            .map(|call| (call, ToolGuardDecision::Allow))
            .collect::<Vec<_>>();

        let ports = registry.build(agent.ctx.clone());
        let catalog_port = ports.catalog_port();
        let execution_port = ports.execution();
        let _ = execute_tool_round(
            &context,
            &calls,
            &catalog_port,
            &execution_port,
            &policy::AllowAllPolicy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            &agent,
            &sink,
            &hook_port,
            &tokio_util::sync::CancellationToken::new(),
            "en",
            &workspace_root,
            &guarded_calls,
            &std::sync::Arc::new(crate::application::interaction::InteractionBridge::new()),
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
    async fn cancelled_tool_round_materializes_one_result_for_each_provider_call() {
        let calls = vec![lifecycle_call(0), lifecycle_call(1)];
        let completed = ToolExecution::new(
            &calls[0],
            ToolOutcome::new("finished", Value::Null, Vec::new()),
        );
        let results = complete_cancelled_tool_round(&calls, vec![completed]);
        let materializer = crate::application::testing::test_tool_result_materializer();

        let message =
            tool_results_for_api(materializer.as_ref(), results, "test-cancelled-round").await;

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
    async fn test_tool_results_for_api_uses_provider_id_not_runtime_id() {
        let results = vec![ToolExecution::from_parts(
            ToolCallId::new_v7(),
            "provider-id".to_string(),
            "Bash".to_string(),
            ToolOutcome::new("ok", serde_json::json!({ "text": "ok" }), Vec::new()),
        )];
        let materializer = crate::application::testing::test_tool_result_materializer();
        let message =
            tool_results_for_api(materializer.as_ref(), results, "test-provider-id").await;

        let [ContentBlock::ToolResult { tool_use_id, .. }] = message.content.as_slice() else {
            panic!("expected one tool result");
        };
        assert_eq!(tool_use_id, "provider-id");
    }

    #[tokio::test]
    async fn test_tool_results_for_api_persists_oversized_tui_result() {
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
        let materializer = crate::application::testing::test_tool_result_materializer();
        let message = tool_results_for_api(materializer.as_ref(), results, &session_id).await;

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

    // ─── #1246: InteractionBridge resolve tests ───

    use super::resolve_ask_user_via_bridge;
    use crate::application::interaction::InteractionBridge;

    fn dummy_interaction_suspension_call(
        id: &str,
    ) -> (ToolCall, tools::ToolSuspension, tools::AuthorizationContext) {
        let call = ToolCall {
            id: ToolCallId::from_legacy_or_new(id),
            provider_id: format!("provider-{id}"),
            name: "AskUserQuestion".to_string(),
            index: 0,
            input: serde_json::json!({"question": "test"}),
        };
        let suspension =
            tools::ToolSuspension::UserInteraction(tools::UserInteractionSpec::new(vec![
                tools::UserQuestion {
                    prompt: "continue?".to_string(),
                    options: vec![],
                    allow_multi: false,
                    allow_free_input: false,
                    default: None,
                },
            ]));
        (call, suspension, tools::AuthorizationContext::STANDARD)
    }

    #[tokio::test]
    async fn bridge_reply_produces_tool_success() {
        let bridge = Arc::new(InteractionBridge::new());
        let sink = RecordingSink::default();
        let hook_port: Arc<dyn HookPort> = Arc::new(NoOpHookPort);
        let cancel = CancellationToken::new();
        let run_id = sdk::RunId::new_v7();
        let (call, suspension, auth) = dummy_interaction_suspension_call("call-reply");
        let calls = [(&call, suspension, auth)];

        let bridge_for_reply = bridge.clone();
        let sink_for_reply = sink.clone();
        let ctx = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let root = std::path::Path::new(".");

        let results = tokio::select! {
            results = resolve_ask_user_via_bridge(
                &ctx,
                &sink,
                &hook_port,
                &calls,
                &cancel,
                root,
                &run_id,
                &bridge,
            ) => results,

            // Auto-reply when InteractionRequested appears.
            _ = async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    let events = sink_for_reply.events.lock().unwrap();
                    if let Some(RuntimeStreamEvent::InteractionRequested { request }) =
                        events.iter().find(|e| {
                            matches!(e, RuntimeStreamEvent::InteractionRequested { .. })
                        })
                    {
                        let request_id = request.id.clone();
                        drop(events);
                        bridge_for_reply.reply(
                          &request_id,
                          sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("yes".into())]),
                      );
                      // Don't return — keep the branch alive so the resolver wins select!.
                      tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                  }
              }
          } => Vec::new(),
        };

        assert_eq!(results.len(), 1);
        assert!(!results[0].outcome.is_error);
        assert!(results[0].outcome.text.contains("yes"));
    }

    #[tokio::test]
    async fn bridge_cancel_produces_tool_error() {
        let bridge = Arc::new(InteractionBridge::new());
        let sink = RecordingSink::default();
        let hook_port: Arc<dyn HookPort> = Arc::new(NoOpHookPort);
        let cancel = CancellationToken::new();
        let run_id = sdk::RunId::new_v7();
        let (call, suspension, auth) = dummy_interaction_suspension_call("call-cancel");
        let calls = [(&call, suspension, auth)];

        let bridge_for_cancel = bridge.clone();
        let sink_for_cancel = sink.clone();
        let ctx = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let root = std::path::Path::new(".");

        let results = tokio::select! {
            results = resolve_ask_user_via_bridge(
                &ctx,
                &sink,
                &hook_port,
                &calls,
                &cancel,
                root,
                &run_id,
                &bridge,
            ) => results,

            _ = async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    let events = sink_for_cancel.events.lock().unwrap();
                    if let Some(RuntimeStreamEvent::InteractionRequested { request }) =
                        events.iter().find(|e| {
                            matches!(e, RuntimeStreamEvent::InteractionRequested { .. })
                        })
                    {
                        let request_id = request.id.clone();
                        drop(events);
                        bridge_for_cancel.cancel(&request_id, sdk::InteractionCancelReason::UserCancelled);
                      // Don't return — keep the branch alive so the resolver wins select!.
                      tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                  }
                }
            } => Vec::new(),
        };

        assert_eq!(results.len(), 1);
        assert!(results[0].outcome.is_error);
        assert!(results[0].outcome.text.contains("Cancelled"));
    }

    #[tokio::test]
    async fn bridge_two_suspensions_serial() {
        let bridge = Arc::new(InteractionBridge::new());
        let sink = RecordingSink::default();
        let hook_port: Arc<dyn HookPort> = Arc::new(NoOpHookPort);
        let cancel = CancellationToken::new();
        let run_id = sdk::RunId::new_v7();
        let (call1, suspension1, auth1) = dummy_interaction_suspension_call("call-1");
        let (call2, suspension2, auth2) = dummy_interaction_suspension_call("call-2");
        let calls = [(&call1, suspension1, auth1), (&call2, suspension2, auth2)];

        let bridge_for_reply = bridge.clone();
        let sink_for_reply = sink.clone();
        let ctx = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let root = std::path::Path::new(".");

        let results = tokio::select! {
            results = resolve_ask_user_via_bridge(
                &ctx,
                &sink,
                &hook_port,
                &calls,
                &cancel,
                root,
                &run_id,
                &bridge,
            ) => results,

            _ = async {
                // Reply to each interaction as it appears.
                for _ in 0..2 {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        let events = sink_for_reply.events.lock().unwrap();
                        let pending = events.iter().rev().find(|e| {
                            if let RuntimeStreamEvent::InteractionRequested { request } = e {
                                bridge_for_reply.contains(&request.id)
                            } else {
                                false
                            }
                        });
                        if let Some(RuntimeStreamEvent::InteractionRequested { request }) = pending {
                            let request_id = request.id.clone();
                            drop(events);
                            bridge_for_reply.reply(
                                &request_id,
                                sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("ok".into())]),
                            );
                            break;
                        }
                    }
                }
                // Keep the auto-reply task alive until resolver finishes.
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            } => Vec::new(),
        };

        assert_eq!(results.len(), 2);
        assert!(!results[0].outcome.is_error);
        assert!(!results[1].outcome.is_error);

        // Verify two InteractionRequested events were emitted (one per call).
        let interaction_count = sink
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, RuntimeStreamEvent::InteractionRequested { .. }))
            .count();
        assert_eq!(interaction_count, 2);
    }
}
