use super::logging::{
    build_json_logger_input_data, build_json_logger_tool_call_data,
    build_json_logger_tool_result_data,
};
use super::progress::{build_tool_calls_progress_event, format_grouped_tool_summaries};
use super::*;
use crate::api::core::config::AgentRoleConfig;
use crate::api::core::message::Message;
use crate::api::core::tool::AgentProgressKind;

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
    let calls = vec![
        test_tool_call(
            "1",
            "Read",
            serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        ),
        test_tool_call(
            "2",
            "Grep",
            serde_json::json!({"pattern": "AgentProgress", "path": "/repo/src"}),
        ),
    ];

    let event = build_tool_calls_progress_event(2, &calls);

    assert_eq!(event.sequence, 2);
    match event.kind {
        AgentProgressKind::ToolCalls { calls } => {
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].id, "1");
            assert_eq!(calls[0].name, "Read");
            assert_eq!(
                calls[0].input,
                serde_json::json!({"file_path": "/repo/src/lib.rs"})
            );
            assert_eq!(calls[0].summary, "src/lib.rs");
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

    assert_eq!(summary, "Read ×4: a.rs, b.rs, c.rs +1 more");
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
    let call = test_tool_call(
        "tool-1",
        "Bash",
        serde_json::json!({"command": "cargo check"}),
    );

    let data = build_json_logger_tool_call_data(&call);

    assert_eq!(data["tool_use_id"], "tool-1");
    assert_eq!(data["tool_name"], "Bash");
    assert_eq!(data["input"]["command"], "cargo check");
}

#[test]
fn test_build_json_logger_tool_result_data_contains_full_output() {
    let mut call_info = std::collections::HashMap::new();
    call_info.insert(
        "tool-1".to_string(),
        ("Read".to_string(), "file.rs".to_string()),
    );

    let data = build_json_logger_tool_result_data("tool-1", "完整输出", false, &call_info);

    assert_eq!(data["tool_use_id"], "tool-1");
    assert_eq!(data["tool_name"], "Read");
    assert_eq!(data["is_error"], false);
    assert_eq!(data["output"], "完整输出");
}

fn test_tool_call(
    id: &str,
    name: &str,
    input: serde_json::Value,
) -> crate::api::core::agent::ToolCall {
    crate::api::core::agent::ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        input,
    }
}
