use super::logging::{
    build_json_logger_input_data, build_json_logger_tool_call_data,
    build_json_logger_tool_result_data,
};
use super::progress::{build_tool_calls_progress_event, format_grouped_tool_summaries};
use super::*;
use async_trait::async_trait;
use provider::api::{LlmError, LlmProvider, StreamResponse, SystemBlock};
use share::config::AgentRoleConfig;
use share::message::Message;
use share::tool::AgentProgressKind;
use std::collections::HashSet;
use std::sync::Arc;
use tools::api::{AgentRunRequest, AgentRunner, ToolExecutionContext};

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
            assert_eq!(calls[0].summary, "");
            assert_eq!(calls[1].name, "Grep");
            assert_eq!(calls[1].summary, "\"AgentProgress\" in src");
        }
        AgentProgressKind::Message { .. } => panic!("expected ToolCalls event"),
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
        AgentProgressKind::ToolCalls { calls } => {
            assert_eq!(calls[0].summary, "cargo check -p aemeath-cli…");
        }
        AgentProgressKind::Message { .. } => panic!("expected ToolCalls event"),
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

#[tokio::test]
async fn test_run_agent_provider_cancelled_error_returns_user_cancelled() {
    let runner = test_runner(LlmError::Cancelled);
    let ctx = test_ctx();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            max_turns: Some(1),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(result, "Cancelled by user");
}

#[tokio::test]
async fn test_run_agent_context_cancelled_after_provider_error_returns_user_cancelled() {
    let runner = test_runner(LlmError::Network("interrupted".to_string()));
    let ctx = test_ctx();
    ctx.cancel.cancel();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            max_turns: Some(1),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(result, "Cancelled by user");
}

#[tokio::test]
async fn test_run_agent_non_cancel_provider_error_returns_sub_agent_error() {
    let runner = test_runner(LlmError::Network("boom".to_string()));
    let ctx = test_ctx();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "prompt",
            system: "system",
            ctx: &ctx,
            max_turns: Some(1),
            model_spec: None,
            progress_tx: None,
        })
        .await;

    assert_eq!(result, "Sub-agent error: network error: boom");
}

fn test_tool_call(
    id: &str,
    name: &str,
    input: serde_json::Value,
) -> crate::business::agent::ToolCall {
    test_tool_call_with_id(sdk::ids::ToolCallId::from_legacy_or_new(id), name, input)
}

fn test_tool_call_with_id(
    id: sdk::ids::ToolCallId,
    name: &str,
    input: serde_json::Value,
) -> crate::business::agent::ToolCall {
    crate::business::agent::ToolCall {
        provider_id: "provider-test".to_string(),
        id,
        name: name.to_string(),
        index: 0,
        input,
    }
}

fn test_runner(error: LlmError) -> CliAgentRunner {
    CliAgentRunner {
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            ErrorProvider { error },
        ))),
        pool: None,
        agents_config: Arc::new(share::config::AgentsConfig::default()),
        hook_runner: hook::api::HookRunner::empty(".".to_string()),
        reasoning: false,
        models_config: Arc::new(share::config::ModelsConfig::default()),
    }
}

fn test_ctx() -> ToolExecutionContext {
    let cwd = std::env::current_dir().unwrap();
    ToolExecutionContext {
        cwd: cwd.clone(),
        workspace: project::api::WorkspaceService::new(cwd),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: Arc::new(std::sync::Mutex::new(HashSet::new())),
        agent_runner: None,
        session_reminders: None,
        memory_config: share::config::MemoryConfig::default(),
        plan_mode: None,
        allow_all: true,
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    }
}

struct ErrorProvider {
    error: LlmError,
}

#[async_trait]
impl LlmProvider for ErrorProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _handler: &mut dyn provider::api::StreamHandler,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<StreamResponse, LlmError> {
        Err(match &self.error {
            LlmError::Network(message) => LlmError::Network(message.clone()),
            LlmError::Api {
                error_type,
                message,
            } => LlmError::Api {
                error_type: error_type.clone(),
                message: message.clone(),
            },
            LlmError::RateLimited => LlmError::RateLimited,
            LlmError::ContextTooLong => LlmError::ContextTooLong,
            LlmError::Cancelled => LlmError::Cancelled,
            LlmError::Stream(message) => LlmError::Stream(message.clone()),
            LlmError::Config(message) => LlmError::Config(message.clone()),
            LlmError::StreamTruncated {
                tool_call_id,
                tool_call_name,
                accumulated_bytes,
                delta_count,
                head_preview,
                tail_preview,
            } => LlmError::StreamTruncated {
                tool_call_id: tool_call_id.clone(),
                tool_call_name: tool_call_name.clone(),
                accumulated_bytes: *accumulated_bytes,
                delta_count: *delta_count,
                head_preview: head_preview.clone(),
                tail_preview: tail_preview.clone(),
            },
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}
