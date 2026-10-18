use super::OpenAICompatibleProvider;
use share::message::{ContentBlock, Message, Role};

#[test]
fn system_message_uses_stable_prefix_without_anthropic_cache_control() {
    let system = "stable instructions";
    let converted = OpenAICompatibleProvider::convert_messages(system, &[], false).unwrap();

    assert_eq!(
        converted,
        vec![serde_json::json!({
            "role": "system",
            "content": "stable instructions"
        })]
    );
    assert!(converted[0].get("cache_control").is_none());
}

#[test]
fn test_convert_messages_preserves_real_reasoning_content_with_tool_calls() {
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Thinking {
                thinking: "需要读取文件".to_string(),
                signature: None,
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path":"/tmp/a"}),
            },
        ],
        metadata: None,
    }];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, true).unwrap();
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
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Thinking {
                thinking: "已有推理内容".to_string(),
                signature: None,
            },
            ContentBlock::Text {
                text: "结论".to_string(),
            },
        ],
        metadata: None,
    }];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, true).unwrap();
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
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"file_path":"/tmp/a"}),
        }],
        metadata: None,
    }];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, false).unwrap();
    let assistant = converted
        .iter()
        .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .unwrap();

    assert!(assistant.get("reasoning_content").is_none());
    assert!(assistant.get("tool_calls").is_some());
}

#[test]
fn test_convert_messages_drops_reasoning_only_assistant() {
    let messages = vec![Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Thinking {
            thinking: "只有推理，没有可见内容".to_string(),
            signature: None,
        }],
        metadata: None,
    }];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, true).unwrap();

    assert!(converted.iter().all(|m| {
        m.get("role").and_then(|v| v.as_str()) != Some("assistant")
            || m.get("content").is_some_and(|v| !v.is_null())
            || m.get("tool_calls").is_some()
    }));
    assert!(converted.is_empty());
}

#[test]
fn test_convert_messages_preserves_all_historical_thinking() {
    let messages = vec![
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "What is 1+1?".to_string(),
            }],
            metadata: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "We need to compute 1+1. The answer is two.".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "two".to_string(),
                },
            ],
            metadata: None,
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "And 2+2?".to_string(),
            }],
            metadata: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "Now compute 2+2. The answer is four.".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "four".to_string(),
                },
            ],
            metadata: None,
        },
    ];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, true).unwrap();
    let assistants: Vec<_> = converted
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .collect();
    assert_eq!(assistants.len(), 2, "应有两条 assistant 消息");
    // 所有轮的 thinking 完整保留
    assert_eq!(
        assistants[0].get("reasoning_content"),
        Some(&serde_json::json!(
            "We need to compute 1+1. The answer is two."
        )),
        "历史轮 thinking 应完整保留"
    );
    assert_eq!(
        assistants[1].get("reasoning_content"),
        Some(&serde_json::json!("Now compute 2+2. The answer is four.")),
        "当前轮 thinking 应完整保留"
    );
}

#[test]
fn test_convert_messages_preserves_historical_thinking_with_tool_calls() {
    let messages = vec![
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "read file".to_string(),
            }],
            metadata: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "需要读取文件".to_string(),
                    signature: None,
                },
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path":"/tmp/a"}),
                },
            ],
            metadata: None,
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "done?".to_string(),
            }],
            metadata: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "当前轮推理".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "done".to_string(),
                },
            ],
            metadata: None,
        },
    ];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, true).unwrap();
    let assistants: Vec<_> = converted
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .collect();
    assert_eq!(assistants.len(), 2);
    // 历史轮 thinking 完整保留，tool_calls 也不受影响
    assert_eq!(
        assistants[0].get("reasoning_content"),
        Some(&serde_json::json!("需要读取文件"))
    );
    assert!(
        assistants[0].get("tool_calls").is_some(),
        "历史轮 tool_calls 应保留"
    );
    // 当前轮完整保留
    assert_eq!(
        assistants[1].get("reasoning_content"),
        Some(&serde_json::json!("当前轮推理"))
    );
}

#[test]
fn test_convert_messages_current_turn_without_thinking_keeps_historical() {
    let messages = vec![
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "1+1?".to_string(),
            }],
            metadata: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "历史推理内容".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "two".to_string(),
                },
            ],
            metadata: None,
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "2+2?".to_string(),
            }],
            metadata: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "four".to_string(),
            }],
            metadata: None,
        },
    ];
    let converted = OpenAICompatibleProvider::convert_messages("", &messages, true).unwrap();
    let assistants: Vec<_> = converted
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .collect();
    assert_eq!(assistants.len(), 2);
    // 历史轮 thinking 完整保留
    assert_eq!(
        assistants[0].get("reasoning_content"),
        Some(&serde_json::json!("历史推理内容"))
    );
    // 当前轮无 thinking 块但 reasoning 开启 → 空字符串（保持 DeepSeek 兼容）
    assert_eq!(
        assistants[1].get("reasoning_content"),
        Some(&serde_json::json!(""))
    );
}

fn tool_result_message_with_image(text: &str, base64: &str, tool_use_id: &str) -> Message {
    // 形状对齐 to_llm_view 输出：image 块 + text 块，`text` 字段置 None。
    Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: serde_json::json!([
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": base64,
                    }
                },
                {"type": "text", "text": text},
            ]),
            is_error: false,
            text: None,
        }],
        metadata: None,
    }
}

fn assistant_tool_use_message(tool_use_ids: &[&str]) -> Message {
    Message {
        role: Role::Assistant,
        content: tool_use_ids
            .iter()
            .map(|id| ContentBlock::ToolUse {
                id: id.to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/a.png"}),
            })
            .collect(),
        metadata: None,
    }
}

#[test]
fn tool_result_images_follow_tool_message_in_user_message() {
    let messages = vec![
        assistant_tool_use_message(&["call_1"]),
        tool_result_message_with_image("Image: /tmp/a.png (10 bytes, image/png)", "QUJD", "call_1"),
    ];

    let converted = OpenAICompatibleProvider::convert_messages("", &messages, false).unwrap();

    let tool_index = converted
        .iter()
        .position(|m| m.get("role").and_then(|v| v.as_str()) == Some("tool"))
        .expect("tool message present");
    let user_index = converted
        .iter()
        .position(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
        .expect("image carrier user message present");
    assert!(tool_index < user_index, "{converted:#?}");
    assert_eq!(
        converted[tool_index]["content"],
        "Image: /tmp/a.png (10 bytes, image/png)"
    );

    let content = converted[user_index]["content"]
        .as_array()
        .expect("user content array");
    let image = content
        .iter()
        .find(|b| b.get("type").and_then(|v| v.as_str()) == Some("image_url"))
        .expect("image_url block present");
    assert_eq!(image["image_url"]["url"], "data:image/png;base64,QUJD");
}

#[test]
fn multiple_tool_result_images_merge_into_one_user_message() {
    let messages = vec![
        assistant_tool_use_message(&["call_1", "call_2"]),
        tool_result_message_with_image("Image: /tmp/a.png", "QUJD", "call_1"),
        tool_result_message_with_image("Image: /tmp/b.png", "REVG", "call_2"),
    ];

    let converted = OpenAICompatibleProvider::convert_messages("", &messages, false).unwrap();

    let user_messages = converted
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
        .collect::<Vec<_>>();
    assert_eq!(user_messages.len(), 1, "{converted:#?}");
    let images = user_messages[0]["content"]
        .as_array()
        .expect("content array")
        .iter()
        .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("image_url"))
        .count();
    assert_eq!(images, 2, "{converted:#?}");
}

#[test]
fn tool_result_without_images_emits_no_extra_user_message() {
    let messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: serde_json::json!("plain output"),
            is_error: false,
            text: None,
        }],
        metadata: None,
    }];

    let converted = OpenAICompatibleProvider::convert_messages("", &messages, false).unwrap();

    assert!(
        converted
            .iter()
            .all(|m| m.get("role").and_then(|v| v.as_str()) != Some("user")),
        "{converted:#?}"
    );
}
