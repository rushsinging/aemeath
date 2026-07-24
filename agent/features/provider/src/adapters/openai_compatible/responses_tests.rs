//! Responses API 请求构造的单元测试（从 responses.rs 内联测试迁出）。
//!
//! 测试覆盖：
//! - `messages_to_responses_input` 的各种消息类型转换
//! - `build_responses_request_body` 的 instructions / tools / cache_control 行为

use super::*;

#[test]
fn test_messages_to_responses_input_user() {
    let messages = vec![Message::user("hello")];
    let input = messages_to_responses_input(&messages);
    assert_eq!(input.len(), 1);
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[0]["content"], "hello");
}

#[test]
fn test_messages_to_responses_input_assistant_with_tool() {
    let messages = vec![Message {
        role: share::message::Role::Assistant,
        content: vec![
            share::message::ContentBlock::Text {
                text: "thinking...".to_string(),
            },
            share::message::ContentBlock::ToolUse {
                id: "call_123".to_string(),
                name: "get_time".to_string(),
                input: serde_json::json!({}),
            },
        ],
        metadata: None,
    }];
    let input = messages_to_responses_input(&messages);
    // text message + function_call
    assert_eq!(input.len(), 2);
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[1]["type"], "function_call");
    assert_eq!(input[1]["name"], "get_time");
    assert_eq!(input[1]["call_id"], "call_123");
}

#[test]
fn test_messages_to_responses_input_tool_result() {
    let messages = vec![Message {
        role: share::message::Role::User,
        content: vec![share::message::ContentBlock::ToolResult {
            tool_use_id: "call_123".to_string(),
            content: serde_json::json!("12:00"),
            is_error: false,
            text: Some("12:00".to_string()),
        }],
        metadata: None,
    }];
    let input = messages_to_responses_input(&messages);
    assert_eq!(input.len(), 1);
    assert_eq!(input[0]["type"], "function_call_output");
    assert_eq!(input[0]["call_id"], "call_123");
    assert_eq!(input[0]["output"], "12:00");
}

#[test]
fn responses_instructions_omit_anthropic_cache_control() {
    use crate::adapters::client::OpenAIProviderConfig;
    use crate::ProviderDriverKind;

    let config = OpenAIProviderConfig::from_driver(ProviderDriverKind::OpenAI, "test");
    let provider = super::super::OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://example.com".to_string()),
        Some("test-model".to_string()),
        8192,
        false,
        None,
        60,
    );
    let scope = InvocationScope::new(
        "test-model",
        8192,
        crate::ports::ReasoningLevel::Off,
        crate::ports::ReasoningLevel::Off,
    )
    .expect("valid scope");
    let system = vec![SystemBlock::cached("stable instructions".to_string())];

    let body = provider.build_responses_request_body(&scope, &system, &[], &[], false);
    assert_eq!(body["instructions"], "stable instructions");
    assert!(body.get("cache_control").is_none());
}

#[test]
fn test_build_responses_request_body_injects_tools_from_flat_schema() {
    use crate::adapters::client::OpenAIProviderConfig;
    use crate::ProviderDriverKind;

    // ToolRegistry::schemas_for 产出 Anthropic 扁平格式（非 function 嵌套）
    let flat_schemas = vec![serde_json::json!({
        "name": "get_weather",
        "description": "Get weather",
        "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}},
    })];

    let config = OpenAIProviderConfig::from_driver(ProviderDriverKind::OpenAI, "test");
    let provider = super::super::OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://example.com".to_string()),
        Some("test-model".to_string()),
        8192,
        false,
        None,
        60,
    );
    let scope = InvocationScope::new(
        "test-model",
        8192,
        crate::ports::ReasoningLevel::Off,
        crate::ports::ReasoningLevel::Off,
    )
    .expect("valid scope");
    let body = provider.build_responses_request_body(&scope, &[], &[], &flat_schemas, false);

    // tools 必须被注入（修复前 schema.get("function") = None 导致 tools 丢失）
    let tools = body.get("tools").expect("tools should be injected");
    assert_eq!(tools.as_array().unwrap().len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["name"], "get_weather");
    assert_eq!(tools[0]["description"], "Get weather");
    assert_eq!(
        tools[0]["parameters"]["properties"]["city"]["type"],
        "string"
    );
}
