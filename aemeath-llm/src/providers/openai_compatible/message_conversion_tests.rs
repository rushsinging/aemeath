use super::{OpenAICompatibleProvider, ReasoningConfig};
use crate::client::OpenAIProviderConfig;
use crate::ApiDriverKind;
use aemeath_core::message::{ContentBlock, Message, Role};

fn provider_with_reasoning() -> OpenAICompatibleProvider {
    OpenAICompatibleProvider::new(
        OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "DeepSeek"),
        "test-key".to_string(),
        None,
        Some("deepseek-v4-pro".to_string()),
        8192,
        true,
        Some(ReasoningConfig::Bool(true)),
    )
}

fn provider_without_reasoning() -> OpenAICompatibleProvider {
    OpenAICompatibleProvider::new(
        OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "DeepSeek"),
        "test-key".to_string(),
        None,
        Some("deepseek-v4-pro".to_string()),
        8192,
        false,
        Some(ReasoningConfig::Bool(false)),
    )
}

#[test]
fn test_convert_messages_preserves_real_reasoning_content_with_tool_calls() {
    let provider = provider_with_reasoning();
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Thinking {
                thinking: "需要读取文件".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path":"/tmp/a"}),
            },
        ],
    }];

    let converted = provider.convert_messages(&[], &messages).unwrap();
    let assistant = converted
        .iter()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .unwrap();

    assert_eq!(
        assistant.get("reasoning_content"),
        Some(&serde_json::json!("需要读取文件"))
    );
    assert!(assistant.get("tool_calls").is_some());
}

#[test]
fn test_convert_messages_preserves_real_reasoning_content_even_when_reasoning_disabled() {
    let provider = provider_without_reasoning();
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Thinking {
                thinking: "已有推理内容".to_string(),
            },
            ContentBlock::Text {
                text: "结论".to_string(),
            },
        ],
    }];

    let converted = provider.convert_messages(&[], &messages).unwrap();
    let assistant = converted
        .iter()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .unwrap();

    assert_eq!(
        assistant.get("reasoning_content"),
        Some(&serde_json::json!("已有推理内容"))
    );
    assert_eq!(assistant.get("content"), Some(&serde_json::json!("结论")));
}

#[test]
fn test_convert_messages_omits_reasoning_content_when_reasoning_disabled() {
    let provider = provider_without_reasoning();
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"file_path":"/tmp/a"}),
        }],
    }];

    let converted = provider.convert_messages(&[], &messages).unwrap();
    let assistant = converted
        .iter()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .unwrap();

    assert!(assistant.get("reasoning_content").is_none());
    assert!(assistant.get("tool_calls").is_some());
}
