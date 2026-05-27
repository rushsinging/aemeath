use super::*;

// ── Helper constructors ────────────────────────────────────────

/// Build an assistant message with ToolUse blocks.
fn assistant_with_tools(ids: &[&str]) -> Message {
    Message {
        role: Role::Assistant,
        content: ids
            .iter()
            .map(|&id| ContentBlock::ToolUse {
                id: id.to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({}),
            })
            .collect(),
    }
}

/// Build an assistant message that is purely text.
fn assistant_text(text: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
    }
}

// ── 1. Message constructors ────────────────────────────────────

#[test]
fn test_user_constructor() {
    let msg = Message::user("hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 1);
    assert!(matches!(
        &msg.content[0],
        ContentBlock::Text { text } if text == "hello"
    ));
}

#[test]
fn test_user_with_image() {
    let msg = Message::user_with_image("caption", "base64data".into(), "image/png".into());
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 2);
    // First block should be Image
    assert!(matches!(
        &msg.content[0],
        ContentBlock::Image {
            source: ImageSource::Base64 { media_type, data },
        } if media_type == "image/png" && data == "base64data"
    ));
    // Second block should be Text
    assert!(matches!(
        &msg.content[1],
        ContentBlock::Text { text } if text == "caption"
    ));
}

#[test]
fn test_user_with_images_multiple() {
    let images = vec![
        ("img1".to_string(), "image/png".to_string()),
        ("img2".to_string(), "image/jpeg".to_string()),
    ];
    let msg = Message::user_with_images("multi caption", images);
    assert_eq!(msg.role, Role::User);
    // 2 Image blocks + 1 Text block = 3
    assert_eq!(msg.content.len(), 3);
    assert!(matches!(&msg.content[0], ContentBlock::Image { .. }));
    assert!(matches!(&msg.content[1], ContentBlock::Image { .. }));
    assert!(matches!(
        &msg.content[2],
        ContentBlock::Text { text } if text == "multi caption"
    ));
}

#[test]
fn test_tool_results_constructor() {
    let msg = Message::tool_results(vec![
        ("tool_1".to_string(), "result1".to_string(), false),
        ("tool_2".to_string(), "error!".to_string(), true),
    ]);
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 2);

    match &msg.content[0] {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tool_1");
            assert_eq!(content, &serde_json::Value::String("result1".to_string()));
            assert!(!is_error);
        }
        _ => panic!("expected ToolResult"),
    }

    match &msg.content[1] {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tool_2");
            assert!(content.as_str().unwrap_or_default().contains("error"));
            assert!(is_error);
        }
        _ => panic!("expected ToolResult"),
    }
}

// ── 2. Query methods ───────────────────────────────────────────

#[test]
fn test_extract_tool_uses_present() {
    let msg = assistant_with_tools(&["t1", "t2"]);
    let uses = msg.extract_tool_uses();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].0, "t1");
    assert_eq!(uses[0].1, "Bash");
    assert_eq!(uses[1].0, "t2");
}

#[test]
fn test_extract_tool_uses_empty() {
    let msg = Message::user("no tools here");
    assert!(msg.extract_tool_uses().is_empty());
}

#[test]
fn test_text_content_single() {
    let msg = Message::user("hello world");
    assert_eq!(msg.text_content(), "hello world");
}

#[test]
fn test_text_content_multiple_blocks() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "part1".to_string(),
            },
            ContentBlock::Text {
                text: "part2".to_string(),
            },
        ],
    };
    assert_eq!(msg.text_content(), "part1part2");
}

#[test]
fn test_text_content_skips_non_text() {
    let msg = Message {
        role: Role::Assistant,
        content: vec![
            ContentBlock::Text {
                text: "before".to_string(),
            },
            ContentBlock::ToolUse {
                id: "x".to_string(),
                name: "Y".to_string(),
                input: serde_json::json!({}),
            },
            ContentBlock::Text {
                text: "after".to_string(),
            },
        ],
    };
    assert_eq!(msg.text_content(), "beforeafter");
}

#[test]
fn test_has_tool_uses_and_tool_results() {
    let text_msg = Message::user("hi");
    assert!(!text_msg.has_tool_uses());
    assert!(!text_msg.has_tool_results());

    let tool_msg = assistant_with_tools(&["a"]);
    assert!(tool_msg.has_tool_uses());
    assert!(!tool_msg.has_tool_results());

    let result_msg = Message::tool_results(vec![("a".into(), "ok".into(), false)]);
    assert!(!result_msg.has_tool_uses());
    assert!(result_msg.has_tool_results());
}

#[test]
fn test_tool_use_ids() {
    let msg = assistant_with_tools(&["id_a", "id_b"]);
    assert_eq!(msg.tool_use_ids(), vec!["id_a", "id_b"]);
}

#[test]
fn test_tool_result_ids() {
    let msg = Message::tool_results(vec![
        ("r1".into(), "ok".into(), false),
        ("r2".into(), "ok".into(), false),
    ]);
    assert_eq!(msg.tool_result_ids(), vec!["r1", "r2"]);
}

