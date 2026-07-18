// summary 已由 TUI 层从 input 参数组装，runtime 不再生成
use crate::application::agent::{Agent, ToolCall, ToolExecution};
use crate::application::chat::looping::agent_calls::execute_agent_calls;
use crate::application::chat::looping::ask_user::ask_user;
use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::non_agent::execute_non_agent;
use crate::application::chat::looping::permissions::evaluate_calls;
use crate::application::chat::looping::tool_fuse::blocked_tool_execution;
use crate::application::chat::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use hook::api::{HookData, ToolHookData};

use crate::application::chat::looping::engine::{DeniedCall, PolicyEngine};
use crate::LOG_TARGET;
use sdk::ids::ToolCallId;
use share::config::hooks::HookEvent;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::ToolOutcome;
use tools::{ToolExecutionContext, ToolRegistry};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tool_round<S>(
    context: &RuntimeTurnContext,
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    allow_all: bool,
    agent: &Agent<'_>,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    cancel: &CancellationToken,
    language: &str,
    workspace_root: &Path,
    guarded_calls: &[(ToolCall, crate::application::loop_engine::ToolGuardDecision)],
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let engine = PolicyEngine::new(allow_all);

    let (approved, denied) = evaluate_calls(tool_calls, registry, &engine);
    let denied_results =
        deny_tool_calls(&denied, sink, context, hook_ui, hook_runner, workspace_root).await;

    let guard_by_id: std::collections::HashMap<_, _> = guarded_calls
        .iter()
        .map(|(call, decision)| (call.id.clone(), decision))
        .collect();
    let mut fused_results = Vec::new();
    let mut fuse_allowed = Vec::new();
    for call in approved {
        match guard_by_id.get(&call.id) {
            Some(crate::application::loop_engine::ToolGuardDecision::SoftBlock { reason }) => {
                send_tool_call_status(sink, context, &call, RuntimeToolCallStatus::Ready).await;
                send_tool_call_status(sink, context, &call, RuntimeToolCallStatus::Running).await;
                let execution = blocked_tool_execution(&call, reason);
                send_tool_result(sink, context, &execution).await;
                fused_results.push(execution);
            }
            Some(crate::application::loop_engine::ToolGuardDecision::Allow) | None => {
                fuse_allowed.push(call)
            }
        }
    }

    let (agent_approved, non_agent_approved): (Vec<ToolCall>, Vec<ToolCall>) =
        fuse_allowed.into_iter().partition(|c| c.name == "Agent");

    let ask_user_results = ask_user(
        context,
        sink,
        hook_ui,
        hook_runner,
        &non_agent_approved,
        workspace_root,
    )
    .await;
    let non_agent_results = execute_non_agent(
        context,
        agent,
        sink,
        hook_ui,
        hook_runner,
        &non_agent_approved,
        language,
        workspace_root,
    )
    .await;
    let agent_results = execute_agent_calls(
        context,
        &agent_approved,
        registry,
        &agent.ctx,
        sink,
        hook_ui,
        hook_runner,
        cancel,
        workspace_root,
    )
    .await;

    ask_user_results
        .into_iter()
        .chain(non_agent_results)
        .chain(agent_results)
        .chain(fused_results)
        .chain(denied_results)
        .collect()
}

async fn deny_tool_calls<S>(
    denied: &[DeniedCall],
    sink: &S,
    context: &RuntimeTurnContext,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let mut denied_results = Vec::new();
    for call in denied {
        log::warn!(
            target: LOG_TARGET,
            "tool call denied by policy: name={}, reason={}, runtime_id={}, provider_id={}",
            call.name, call.reason, call.id, call.provider_id,
        );
        let _ = hook_ui
            .run_plain(
                hook_runner,
                HookEvent::PermissionDenied,
                Some(&call.name),
                HookData::Permission(hook::api::PermissionHookData {
                    tool_name: call.name.clone(),
                    permission_rule: "deny".to_string(),
                }),
                workspace_root,
            )
            .await;
        // 发送 ToolCall 事件，让 pending 占位行获取 LLM 的 tool_use_id，
        // 后续 ToolResult 中的 mark_tool_header_done 才能精确匹配（Bug #52）。
        let call_id = sdk::ids::ToolCallId::from_legacy_or_new(&call.id);
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCallUpdate {
                context: context.clone(),
                id: call_id.clone(),
                provider_id: None,
                name: call.name.clone(),
                index: 0,
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
            call.provider_id.clone(),
            call.name.clone(),
            outcome,
        );
        send_tool_result(sink, context, &execution).await;
        denied_results.push(execution);
    }
    denied_results
}

pub(crate) async fn run_post_tool_hooks<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    call: &ToolCall,
    execution: &ToolExecution,
    workspace_root: &Path,
    ctx: &ToolExecutionContext,
) where
    S: ChatEventSink,
{
    let output = &execution.outcome.text;
    let is_error = execution.outcome.is_error;
    emit_json_hook_context(
        sink,
        hook_ui
            .run_json_with_cancel(
                hook_runner,
                HookEvent::PostToolUse,
                Some(&call.name),
                HookData::Tool(ToolHookData {
                    tool_name: call.name.clone(),
                    tool_input: call.input.clone(),
                    tool_output: Some(output.to_string()),
                    is_error: Some(is_error),
                }),
                workspace_root,
                &ctx.cancel,
            )
            .await,
    )
    .await;
    if is_error {
        emit_json_hook_context(
            sink,
            hook_ui
                .run_json_with_cancel(
                    hook_runner,
                    HookEvent::PostToolUseFailure,
                    Some(&call.name),
                    HookData::Tool(ToolHookData {
                        tool_name: call.name.clone(),
                        tool_input: call.input.clone(),
                        tool_output: Some(output.to_string()),
                        is_error: Some(is_error),
                    }),
                    workspace_root,
                    &ctx.cancel,
                )
                .await,
        )
        .await;
    }
}

pub(crate) async fn emit_json_hook_context<S>(
    sink: &S,
    hook_results: Vec<(
        share::config::hooks::HookEntry,
        hook::api::HookResult,
        Option<hook::api::HookJsonOutput>,
    )>,
) where
    S: ChatEventSink,
{
    for (_entry, _result, json_output) in hook_results {
        if let Some(json) = json_output {
            if let Some(ctx) = json.additional_context {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(ctx))
                    .await;
            }
            if let Some(msg) = json.system_message {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(msg))
                    .await;
            }
        }
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
        target: LOG_TARGET,
        "tool_results_for_api: {} typed ToolExecution(s) → wire ({} error)",
        results.len(),
        error_count
    );
    let provider_results: Vec<_> = results
        .into_iter()
        .map(|ex| {
            (
                ex.provider_id,
                ex.outcome.text,
                ex.outcome.data,
                ex.outcome.is_error,
                ex.outcome.images,
            )
        })
        .collect();
    materializer
        .materialize_provider_results(session_id, provider_results)
        .await
}

pub(crate) fn log_tool_result(id: &ToolCallId, tool_name: &str, is_error: bool, output: &str) {
    let tr_data = serde_json::json!({
        "tool_use_id": id.to_string(),
        "tool_name": tool_name,
        "is_error": is_error,
        "output": output,
    });
    log::debug!(
        target: LOG_TARGET,
        "tool_result: {}",
        serde_json::to_string(&tr_data).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::{execute_tool_round, tool_results_for_api};
    use crate::application::agent::{Agent, ToolCall, ToolExecution};
    use crate::application::chat::looping::hook_ui::HookUi;
    use crate::application::chat::looping::{
        ChatEventSink, EventFuture, RuntimeStreamEvent, RuntimeTurnContext,
    };
    use crate::application::loop_engine::ToolGuardDecision;
    use async_trait::async_trait;
    use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
    use serde_json::Value;
    use share::message::ContentBlock;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use tools::ToolOutcome;
    use tools::{ToolExecutionContext, ToolRegistry, TypedTool, TypedToolResult};

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
        let cwd = std::env::current_dir().unwrap();
        ToolExecutionContext {
            resources: tools::ToolResources {
                agent_runner: None,
                registry: None,
                memory: Arc::new(memory::NoOpMemory),
                memory_config: share::config::MemoryConfig::default(),
                lang: "en".to_string(),
                allow_all: true,
            },
            workspace: project::wire_production_workspace(cwd)
                .expect("workspace 初始化成功")
                .into_views(),
            run_id: sdk::RunId::new_v7().to_string(),
            cancel: tokio_util::sync::CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            session_reminders: None,
            plan_mode: None,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        }
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
    async fn test_non_concurrency_safe_tools_emit_running_after_previous_result() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register(UnsafeLifecycleTool);
        let ctx = test_tool_context();
        let agent = Agent {
            registry: registry.as_ref(),
            ctx,
        };
        let sink = RecordingSink::default();
        let hook_ui = HookUi::new(sink.clone());
        let hook_runner = hook::api::HookRunner::new(Default::default());
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let workspace_root = std::env::current_dir().unwrap();
        let calls = vec![lifecycle_call(0), lifecycle_call(1)];
        let guarded_calls = calls
            .iter()
            .cloned()
            .map(|call| (call, ToolGuardDecision::Allow))
            .collect::<Vec<_>>();

        let _ = execute_tool_round(
            &context,
            &calls,
            &registry,
            true,
            &agent,
            &sink,
            &hook_ui,
            &hook_runner,
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
}
