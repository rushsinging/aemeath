use super::{apply_message_cache_breakpoint, convert_messages, sanitize_tool_schemas};
use share::message::{ContentBlock, ImageSource, Message, MessageMetadata, MessageSource, Role};

#[test]
fn strips_data_schema_and_keeps_allowed_fields() {
    let schemas = vec![serde_json::json!({
        "name": "Read",
        "description": "Read a file",
        "input_schema": {"type": "object"},
        "data_schema": {"type": "object"},
        "cache_control": {"type": "ephemeral"}
    })];
    let result = sanitize_tool_schemas(&schemas);
    assert_eq!(result.len(), 1);
    let tool = &result[0];
    assert!(tool.get("name").is_some());
    assert!(tool.get("description").is_some());
    assert!(tool.get("input_schema").is_some());
    assert!(tool.get("cache_control").is_some());
    assert!(
        tool.get("data_schema").is_none(),
        "data_schema must be stripped"
    );
}

#[test]
fn preserves_input_schema_content_intact() {
    let input = serde_json::json!({
        "type": "object",
        "properties": {
            "file_path": {"type": "string"}
        },
        "required": ["file_path"]
    });
    let schemas = vec![serde_json::json!({
        "name": "Read",
        "description": "Read",
        "input_schema": input.clone(),
        "data_schema": {"type": "object"},
    })];
    let result = sanitize_tool_schemas(&schemas);
    assert_eq!(result[0].get("input_schema").unwrap(), &input);
}

#[test]
fn handles_empty_schemas() {
    let result = sanitize_tool_schemas(&[]);
    assert!(result.is_empty());
}

// --- convert_messages tests ---

#[test]
fn convert_messages_strips_metadata() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "hi".to_string(),
        }],
        metadata: Some(MessageMetadata {
            source: MessageSource::SystemGenerated,
            hook_notice: None,
            skill_request: None,
            created_at: None,
            system_reminder: false,
        }),
    };
    let result = convert_messages(&[msg]);
    assert_eq!(result.len(), 1);
    assert!(
        result[0].get("metadata").is_none(),
        "metadata must be stripped"
    );
    assert_eq!(result[0]["role"], "user");
}

#[test]
fn convert_messages_text_block() {
    let msg = Message::user("hello world");
    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];
    assert_eq!(block["type"], "text");
    assert_eq!(block["text"], "hello world");
}

#[test]
fn convert_messages_image_strips_placeholder() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".to_string(),
                data: "abc123".to_string(),
            },
            placeholder: Some("[Image #1]".to_string()),
        }],
        metadata: None,
    };
    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "base64");
    assert_eq!(block["source"]["media_type"], "image/png");
    assert_eq!(block["source"]["data"], "abc123");
    assert!(
        block.get("placeholder").is_none(),
        "placeholder must be stripped"
    );
}

#[test]
fn convert_messages_tool_use() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "tu_1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"file_path": "/tmp/a"}),
        }],
        metadata: None,
    };
    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];
    assert_eq!(block["type"], "tool_use");
    assert_eq!(block["id"], "tu_1");
    assert_eq!(block["name"], "Read");
    assert_eq!(block["input"]["file_path"], "/tmp/a");
}

#[test]
fn convert_messages_tool_result_strips_text_field() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: serde_json::json!("done"),
            is_error: false,
            text: Some("done".to_string()),
        }],
        metadata: None,
    };
    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];
    assert_eq!(block["type"], "tool_result");
    assert_eq!(block["tool_use_id"], "tu_1");
    assert_eq!(block["content"], "done");
    assert_eq!(block["is_error"], false);
    assert!(block.get("text").is_none(), "text field must be stripped");
}

#[test]
fn convert_messages_tool_result_with_structured_content_uses_text_first() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: serde_json::json!({"stdout": "structured output"}),
            is_error: false,
            text: Some("plain output".to_string()),
        }],
        metadata: None,
    };

    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];

    assert_eq!(block["content"], "plain output");
    assert!(block["content"].is_string());
}

#[test]
fn convert_messages_legacy_object_tool_result_serializes_content() {
    let msg = Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tu_1".to_string(),
            content: serde_json::json!({"stdout": "legacy output"}),
            is_error: false,
            text: None,
        }],
        metadata: None,
    };

    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];

    assert_eq!(block["content"], r#"{"stdout":"legacy output"}"#);
    assert!(block["content"].is_string());
}

#[test]
fn convert_messages_thinking_block_without_signature_is_stripped() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Thinking {
                thinking: "let me think".to_string(),
                signature: None,
            },
            ContentBlock::Text {
                text: "answer".to_string(),
            },
        ],
        metadata: None,
    };
    let result = convert_messages(&[msg]);
    let content = result[0]["content"].as_array().unwrap();
    // 无 signature 的 thinking block 被剥离，只保留 text
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
}

#[test]
fn convert_messages_thinking_block_with_signature_preserved() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Thinking {
            thinking: "let me think".to_string(),
            signature: Some("sig_abc".to_string()),
        }],
        metadata: None,
    };
    let result = convert_messages(&[msg]);
    let block = &result[0]["content"][0];
    assert_eq!(block["type"], "thinking");
    assert_eq!(block["thinking"], "let me think");
    assert_eq!(block["signature"], "sig_abc");
}

#[test]
fn convert_messages_assistant_role() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "ok".to_string(),
        }],
        metadata: None,
    };
    let result = convert_messages(&[msg]);
    assert_eq!(result[0]["role"], "assistant");
}

// --- apply_message_cache_breakpoint tests ---

#[test]
fn cache_breakpoint_injected_on_penultimate_message() {
    let messages = vec![
        Message::user("first"),
        Message::user("second"),
        Message::user("third"),
    ];
    let mut api = convert_messages(&messages);
    apply_message_cache_breakpoint(&mut api);

    // penultimate = index 1
    let penultimate_content = api[1]["content"].as_array().unwrap();
    let last_block = penultimate_content.last().unwrap();
    assert_eq!(last_block["cache_control"]["type"], "ephemeral");

    // last message (index 2) should NOT have cache_control
    let last_content = api[2]["content"].as_array().unwrap();
    assert!(
        last_content.last().unwrap().get("cache_control").is_none(),
        "last message must not have cache_control"
    );
}

#[test]
fn cache_breakpoint_single_message_noop() {
    let messages = vec![Message::user("only")];
    let mut api = convert_messages(&messages);
    apply_message_cache_breakpoint(&mut api);

    let content = api[0]["content"].as_array().unwrap();
    assert!(
        content.last().unwrap().get("cache_control").is_none(),
        "single message should not get cache_control"
    );
}

#[test]
fn cache_breakpoint_empty_messages_noop() {
    let mut api: Vec<serde_json::Value> = vec![];
    apply_message_cache_breakpoint(&mut api);
    assert!(api.is_empty());
}

#[test]
fn cache_breakpoint_two_messages_hits_first() {
    let messages = vec![Message::user("a"), Message::user("b")];
    let mut api = convert_messages(&messages);
    apply_message_cache_breakpoint(&mut api);

    // penultimate = index 0
    let content = api[0]["content"].as_array().unwrap();
    assert_eq!(
        content.last().unwrap()["cache_control"]["type"],
        "ephemeral"
    );
}
