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
        metadata: None,
    }
}

// ── 1. Message constructors ────────────────────────────────────

#[test]
fn stop_hook_feedback_carries_distinct_message_source() {
    let message = Message::stop_hook_feedback(
        "blocked",
        StopHookFeedback {
            summary: "blocked".to_string(),
            command: "check-agent-stop.sh".to_string(),
            exit_code: Some(2),
            reason: "exit code 2".to_string(),
            stdout_preview: String::new(),
            stderr_preview: "blocked".to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
            output_file: None,
        },
    );

    assert_eq!(
        message.metadata.as_ref().map(|metadata| metadata.source),
        Some(MessageSource::StopHook)
    );
}

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
    // 单图 + 无占位符 → 头尾拆块（向后兼容）
    let msg = Message::user_with_image("caption", "base64data".into(), "image/png".into());
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 2);
    // First block should be Image
    assert!(matches!(
        &msg.content[0],
        ContentBlock::Image {
            source: ImageSource::Base64 { media_type, data },
            ..
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
    // 占位符插入文本中，期望按出现顺序穿插
    let images = vec![
        (
            "[Image #1]".to_string(),
            "img1".to_string(),
            "image/png".to_string(),
        ),
        (
            "[Image #2]".to_string(),
            "img2".to_string(),
            "image/jpeg".to_string(),
        ),
    ];
    let msg = Message::user_with_images("multi caption", images);
    assert_eq!(msg.role, Role::User);
    // 占位符在文本中不存在时，所有 image 堆到尾部、text 在最末
    assert_eq!(msg.content.len(), 3);
    assert!(matches!(&msg.content[0], ContentBlock::Image { .. }));
    assert!(matches!(&msg.content[1], ContentBlock::Image { .. }));
    assert!(matches!(
        &msg.content[2],
        ContentBlock::Text { text } if text == "multi caption"
    ));
}

/// #fix-tui-image-input-output：text 含占位符时，image 按出现顺序穿插拆块。
#[test]
fn test_user_with_images_interleaves_by_placeholder() {
    let images = vec![
        (
            "[Image #1]".to_string(),
            "a".to_string(),
            "image/png".to_string(),
        ),
        (
            "[Image #2]".to_string(),
            "b".to_string(),
            "image/jpeg".to_string(),
        ),
    ];
    // text 中 [Image #2] 在 [Image #1] 前
    let msg = Message::user_with_images("B: [Image #2], A: [Image #1]", images);
    assert_eq!(msg.role, Role::User);
    let placeholders: Vec<String> = msg
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Image {
                placeholder: Some(p),
                ..
            } => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        placeholders,
        vec!["[Image #2]".to_string(), "[Image #1]".to_string()],
        "image 应按 text 中 `[Image #N]` 出现顺序穿插，实际 blocks={:?}",
        msg.content
    );
    // text_content 拼回完整文本
    assert_eq!(msg.text_content(), "B: [Image #2], A: [Image #1]");
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
            ..
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
            ..
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
        metadata: None,
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
        metadata: None,
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
