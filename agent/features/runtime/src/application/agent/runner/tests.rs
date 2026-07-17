use super::logging::{
    build_json_logger_input_data, build_json_logger_tool_call_data,
    build_json_logger_tool_result_data,
};
use super::progress::build_tool_calls_progress_event;
use super::*;
use async_trait::async_trait;
use provider::{InvocationStream, LlmProvider, ProviderError, ProviderErrorKind, SystemBlock};
use share::config::AgentRoleConfig;
use share::message::Message;
use share::tool::AgentProgressKind;
use std::collections::HashSet;
use std::sync::Arc;
use tools::api::{AgentRunRequest, AgentRunner, ToolExecutionContext};

fn format_grouped_tool_summaries(tool_calls: &[crate::application::agent::ToolCall]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for call in tool_calls {
        if let Some(entry) = counts
            .iter_mut()
            .find(|(name, _)| *name == call.name.as_str())
        {
            entry.1 += 1;
        } else {
            counts.push((call.name.as_str(), 1));
        }
    }

    counts
        .into_iter()
        .map(|(name, count)| {
            if count > 1 {
                format!("{name} ×{count}")
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn test_role_max_tokens_override() {
    let role = AgentRoleConfig {
        max_tokens: Some(8192),
        ..Default::default()
    };
    assert_eq!(
        CliAgentRunner::role_max_tokens_override(Some(&role)),
        Some(8192)
    );

    let role = AgentRoleConfig {
        max_tokens: Some(0),
        ..Default::default()
    };
    assert_eq!(CliAgentRunner::role_max_tokens_override(Some(&role)), None);

    let role = AgentRoleConfig {
        max_tokens: None,
        ..Default::default()
    };
    assert_eq!(CliAgentRunner::role_max_tokens_override(Some(&role)), None);

    assert_eq!(CliAgentRunner::role_max_tokens_override(None), None);
}

#[test]
fn test_build_tool_calls_progress_event_preserves_call_data_and_summaries() {
    let id1 = sdk::ids::ToolCallId::new_v7();
    let id2 = sdk::ids::ToolCallId::new_v7();
    let calls = vec![
        test_tool_call_with_id(
            id1.clone(),
            "Read",
            serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        ),
        test_tool_call_with_id(
            id2.clone(),
            "Grep",
            serde_json::json!({"pattern": "AgentProgress", "path": "/repo/src"}),
        ),
    ];

    let event = build_tool_calls_progress_event(2, &calls);

    assert_eq!(event.sequence, 2);
    match event.kind {
        AgentProgressKind::ToolCalls { calls } => {
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].id, id1.to_string());
            assert_eq!(calls[0].name, "Read");
            assert_eq!(
                calls[0].input,
                serde_json::json!({"file_path": "/repo/src/lib.rs"})
            );
            // Read tool 的 summary 为空字符串，TUI 层自己组装

            assert_eq!(calls[1].name, "Grep");
            // 所有 tool 的 summary 为空，TUI 层自己组装
        }
        AgentProgressKind::Message { .. }
        | AgentProgressKind::Started { .. }
        | AgentProgressKind::ToolOutput { .. } => {
            panic!("expected ToolCalls event")
        }
    }
}

#[test]
fn test_build_tool_calls_progress_event_truncates_long_read_groups_at_summary_level() {
    let calls = vec![test_tool_call(
        "1",
        "Bash",
        serde_json::json!({"command": "cargo check -p aemeath-cli && cargo test"}),
    )];

    let event = build_tool_calls_progress_event(1, &calls);

    match event.kind {
        AgentProgressKind::ToolCalls { calls: _ } => {
            // 所有 tool 的 summary 为空
        }
        AgentProgressKind::Message { .. }
        | AgentProgressKind::Started { .. }
        | AgentProgressKind::ToolOutput { .. } => {
            panic!("expected ToolCalls event")
        }
    }
}

#[test]
fn test_format_grouped_tool_summaries_keeps_existing_display_format() {
    let calls = vec![
        test_tool_call("1", "Read", serde_json::json!({"file_path": "/repo/a.rs"})),
        test_tool_call("2", "Read", serde_json::json!({"file_path": "/repo/b.rs"})),
        test_tool_call("3", "Read", serde_json::json!({"file_path": "/repo/c.rs"})),
        test_tool_call("4", "Read", serde_json::json!({"file_path": "/repo/d.rs"})),
    ];

    let summary = format_grouped_tool_summaries(&calls);

    // Read tool 的 summary 为空字符串，不显示详情
    assert_eq!(summary, "Read ×4");
}

#[test]
fn test_build_json_logger_input_data_includes_latest_message_and_schema_names() {
    let messages = vec![Message::user("first"), Message::user("latest")];
    let schemas = vec![serde_json::json!({"name": "Read"})];

    let data = build_json_logger_input_data(&messages, 2, &schemas);

    assert_eq!(data["system_blocks_count"], 2);
    assert_eq!(data["tool_schemas_count"], 1);
    assert_eq!(data["tool_schemas_names"], serde_json::json!(["Read"]));
    assert_eq!(data["messages"].as_array().unwrap().len(), 1);
    assert_eq!(data["messages"][0]["role"], "user");
    assert_eq!(data["messages"][0]["block_count"], 1);
}

#[test]
fn test_build_json_logger_tool_call_data_contains_full_input() {
    let call_id = sdk::ids::ToolCallId::new_v7();
    let call = test_tool_call_with_id(
        call_id.clone(),
        "Bash",
        serde_json::json!({"command": "cargo check"}),
    );

    let data = build_json_logger_tool_call_data(&call);

    assert_eq!(data["tool_use_id"], call_id.to_string());
    assert_eq!(data["tool_name"], "Bash");
    assert_eq!(data["input"]["command"], "cargo check");
}

#[test]
fn test_build_json_logger_tool_result_data_contains_full_output() {
    let tool_id = sdk::ids::ToolCallId::new_v7();
    let mut call_info = std::collections::HashMap::new();
    call_info.insert(tool_id.clone(), ("Read".to_string(), "file.rs".to_string()));

    let data = build_json_logger_tool_result_data(&tool_id, "完整输出", false, &call_info);

    assert_eq!(data["tool_use_id"], tool_id.to_string());
    assert_eq!(data["tool_name"], "Read");
    assert_eq!(data["is_error"], false);
    assert_eq!(data["output"], "完整输出");
}

#[test]
fn test_sub_run_cancellation_scope_is_one_way() {
    let parent = tokio_util::sync::CancellationToken::new();
    let child = parent.child_token();

    child.cancel();
    assert!(child.is_cancelled());
    assert!(
        !parent.is_cancelled(),
        "child cancellation must not cancel parent"
    );

    let second_child = parent.child_token();
    parent.cancel();
    assert!(
        second_child.is_cancelled(),
        "parent cancellation must reach child"
    );
}

#[tokio::test]
async fn test_sub_run_registers_and_clears_active_run_on_registry_cancel() {
    let calls = Arc::new(std::sync::Mutex::new(0usize));
    let registry = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
    let mut runner = test_runner_with_blocking_provider(calls.clone());
    runner.active_run = registry.clone();
    let ctx = test_ctx();

    let driver_registry = registry.clone();
    let driver_calls = calls.clone();
    let driver = tokio::spawn(async move {
        loop {
            if *driver_calls.lock().unwrap() >= 1 {
                let ids = driver_registry.active_ids();
                if let Some(run_id) = ids.first() {
                    assert_eq!(
                        driver_registry.cancel(run_id),
                        sdk::CancelRunOutcome::Accepted
                    );
                    return;
                }
            }
            tokio::task::yield_now().await;
        }
    });

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    driver.await.unwrap();
    assert_eq!(result, tools::api::AgentRunTerminal::Cancelled);
    assert!(
        !ctx.cancel.is_cancelled(),
        "按 Sub Run ID 取消不得反向取消父 Run token"
    );
    assert!(registry.active_ids().is_empty());
}

#[tokio::test]
async fn test_run_agent_provider_cancelled_error_returns_user_cancelled() {
    let runner = test_runner(ProviderError::cancelled());
    let ctx = test_ctx();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(result, tools::api::AgentRunTerminal::Cancelled);
}

#[tokio::test]
async fn test_run_agent_context_cancelled_after_provider_error_returns_user_cancelled() {
    let runner = test_runner(ProviderError::retryable(
        ProviderErrorKind::Network,
        "interrupted",
    ));
    let ctx = test_ctx();
    ctx.cancel.cancel();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(result, tools::api::AgentRunTerminal::Cancelled);
}

#[tokio::test]
async fn test_run_agent_cancel_arrives_mid_flight_during_stream_returns_promptly() {
    // 复现真实场景：cancel 在 sub-agent 正阻塞于 stream_message（真实进行中的 LLM 调用）
    // 时才到达——而不是调用前已取消、也不是 provider 立刻返回 Cancelled。
    // 之前的两个测试都只覆盖了「调用前」的两种情形，没有覆盖「调用中」，
    // 而用户实际点击停止时，sub-agent 几乎总是正阻塞在某次 stream_message 里。
    let calls = Arc::new(std::sync::Mutex::new(0usize));
    let runner = test_runner_with_blocking_provider(calls.clone());
    let cwd = std::env::current_dir().unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let ctx = ToolExecutionContext {
        resources: tools::api::ToolResources {
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
        cancel: cancel.clone(),
        read_files: Arc::new(std::sync::Mutex::new(HashSet::new())),
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    };

    let canceller_calls = calls.clone();
    let canceller = tokio::spawn(async move {
        // 等 stream_message 真正开始阻塞后再取消，确保取消落在「调用进行中」。
        loop {
            if *canceller_calls.lock().unwrap() >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        cancel.cancel();
    });

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runner.run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: None,
        }),
    )
    .await
    .expect("run_agent 必须在 mid-flight cancel 后及时返回，不能挂起等待 provider 自然结束");

    canceller.await.unwrap();
    assert_eq!(result, tools::api::AgentRunTerminal::Cancelled);
}

// issue #646：SubAgentRun emit Started 事件测试
#[tokio::test]
async fn test_started_event_emitted_with_role_and_model() {
    use share::tool::{AgentProgressEvent, AgentProgressKind};
    use tokio::sync::mpsc;

    let runner = test_runner(ProviderError::retryable(
        ProviderErrorKind::Network,
        "setup-only",
    ));
    let ctx = test_ctx();

    let (tx, mut rx) = mpsc::channel::<AgentProgressEvent>(8);

    // model_spec = Some("coder") → role=Some("coder"), resolved_spec 取决于配置（默认 None）
    let _ = runner
        .run_agent(AgentRunRequest {
            prompt: "p",
            system: "s",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: Some("coder"),
            progress_tx: Some(tx),
        })
        .await;

    let ev = rx.recv().await.expect("should receive Started event");
    match ev.kind {
        AgentProgressKind::Started { role, model } => {
            assert_eq!(role.as_deref(), Some("coder"));
            // model_spec="coder" 但 roles 配置无 "coder" → resolve_model_spec 原样返回
            assert_eq!(model, "coder");
        }
        other => panic!("expected Started, got {other:?}"),
    }
}

#[tokio::test]
async fn test_started_event_without_role_uses_main_agent_model() {
    use share::tool::{AgentProgressEvent, AgentProgressKind};
    use tokio::sync::mpsc;

    let runner = test_runner(ProviderError::retryable(
        ProviderErrorKind::Network,
        "setup-only",
    ));
    let ctx = test_ctx();

    let (tx, mut rx) = mpsc::channel::<AgentProgressEvent>(8);

    // model_spec = None → role=None, model=client.model_name()="test-model"
    let _ = runner
        .run_agent(AgentRunRequest {
            prompt: "p",
            system: "s",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: Some(tx),
        })
        .await;

    let ev = rx.recv().await.expect("should receive Started event");
    match ev.kind {
        AgentProgressKind::Started { role, model } => {
            assert!(role.is_none(), "role should be None when not configured");
            assert_eq!(
                model, "test-model",
                "model should fallback to main agent's model"
            );
        }
        other => panic!("expected Started, got {other:?}"),
    }
}

#[tokio::test]
async fn test_started_event_not_emitted_without_progress_tx() {
    // progress_tx = None → 不会 emit（也不会 panic）
    let runner = test_runner(ProviderError::retryable(
        ProviderErrorKind::Network,
        "setup-only",
    ));
    let ctx = test_ctx();

    // 不传 progress_tx，run_agent 应正常完成（即使 setup 内 try_send 被跳过）
    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "p",
            system: "s",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    // ErrorProvider 会返回 Err，但不应 panic
    assert!(matches!(
        result,
        tools::api::AgentRunTerminal::Failed { ref error }
            if error.contains("setup-only") || error.contains("error") || !error.is_empty()
    ));
}

#[tokio::test]
async fn test_run_agent_non_cancel_provider_error_returns_sub_agent_error() {
    let runner = test_runner(ProviderError::fatal(ProviderErrorKind::Network, "boom"));
    let ctx = test_ctx();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            timeout: std::time::Duration::from_secs(30),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(
        result,
        tools::api::AgentRunTerminal::Failed {
            error: "loop adapter error: network error: boom".to_string(),
        }
    );
}

#[tokio::test]
async fn test_run_agent_timeout_comes_from_request_and_returns_typed_failure() {
    let runner = test_runner(ProviderError::retryable(
        ProviderErrorKind::Network,
        "should not be invoked",
    ));
    let ctx = test_ctx();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            timeout: std::time::Duration::from_nanos(1),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(
        result,
        tools::api::AgentRunTerminal::Failed {
            error: "run timed out after 0 seconds".to_string(),
        }
    );
}

fn test_tool_call(
    id: &str,
    name: &str,
    input: serde_json::Value,
) -> crate::application::agent::ToolCall {
    test_tool_call_with_id(sdk::ids::ToolCallId::from_legacy_or_new(id), name, input)
}

fn test_tool_call_with_id(
    id: sdk::ids::ToolCallId,
    name: &str,
    input: serde_json::Value,
) -> crate::application::agent::ToolCall {
    crate::application::agent::ToolCall {
        provider_id: "provider-test".to_string(),
        id,
        name: name.to_string(),
        index: 0,
        input,
    }
}

fn test_runner(error: ProviderError) -> CliAgentRunner {
    CliAgentRunner {
        client: Arc::new(provider::LlmClient::from_provider(Arc::new(
            ErrorProvider { error },
        ))),
        pool: None,
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        agents_config: Arc::new(share::config::AgentsConfig::default()),
        hook_runner: hook::api::HookRunner::empty(),
        reasoning: false,
        models_config: Arc::new(share::config::ModelsConfig::default()),
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
    }
}

fn test_runner_with_blocking_provider(calls: Arc<std::sync::Mutex<usize>>) -> CliAgentRunner {
    CliAgentRunner {
        client: Arc::new(provider::LlmClient::from_provider(Arc::new(
            BlockingThenCancelledProvider { calls },
        ))),
        pool: None,
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        agents_config: Arc::new(share::config::AgentsConfig::default()),
        hook_runner: hook::api::HookRunner::empty(),
        reasoning: false,
        models_config: Arc::new(share::config::ModelsConfig::default()),
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
    }
}

/// 模拟真实进行中的 LLM 流：`invocation_stream` 阻塞在 `cancel.cancelled()` 上，
/// 而不是立刻返回，用于复现「cancel 在调用进行中才到达」的场景。
struct BlockingThenCancelledProvider {
    calls: Arc<std::sync::Mutex<usize>>,
}

#[async_trait]
impl LlmProvider for BlockingThenCancelledProvider {
    async fn invocation_stream(
        &self,
        _scope: &provider::InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        {
            let mut guard = self.calls.lock().unwrap();
            *guard += 1;
        }
        cancel.cancelled().await;
        Err(ProviderError::cancelled())
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

fn test_ctx() -> ToolExecutionContext {
    let cwd = std::env::current_dir().unwrap();
    ToolExecutionContext {
        resources: tools::api::ToolResources {
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

struct ErrorProvider {
    error: ProviderError,
}

#[async_trait]
impl LlmProvider for ErrorProvider {
    async fn invocation_stream(
        &self,
        _scope: &provider::InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        Err(self.error.clone())
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}
