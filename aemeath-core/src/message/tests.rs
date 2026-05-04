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

// ── 3. sanitize_messages ───────────────────────────────────────

#[test]
fn test_sanitize_clean_messages_unchanged() {
    let mut msgs = vec![
        Message::user("hello"),
        assistant_text("hi there"),
        Message::user("how are you"),
    ];
    let len_before = msgs.len();
    sanitize_messages(&mut msgs);
    assert_eq!(msgs.len(), len_before);
}

#[test]
fn test_sanitize_removes_trailing_orphaned_tool_use() {
    let mut msgs = vec![
        Message::user("go"),
        assistant_with_tools(&["tool_1"]),
        // no user message with ToolResult for tool_1
    ];
    sanitize_messages(&mut msgs);
    // The trailing assistant with ToolUse should be removed
    assert!(msgs.last().unwrap().role == Role::User);
    assert_eq!(msgs.len(), 1);
}

#[test]
fn test_sanitize_removes_trailing_orphaned_tool_result() {
    let mut msgs = vec![
        Message::user("start"),
        assistant_with_tools(&["tool_1"]),
        // This ToolResult references a ghost tool, NOT tool_1.
        // tool_1 is still unresolved, so sanitize will strip from the end.
        Message::tool_results(vec![("ghost_tool".into(), "result".into(), false)]),
    ];
    sanitize_messages(&mut msgs);
    // Both the ToolResult msg and the assistant with unresolved tool_1 are removed
    assert!(msgs.last().unwrap().role == Role::User);
}

#[test]
fn test_sanitize_preserves_complete_tool_pairs_in_middle() {
    let mut msgs = vec![
        Message::user("start"),
        assistant_with_tools(&["tool_1"]),
        Message::tool_results(vec![("tool_1".into(), "ok".into(), false)]),
        assistant_text("final"),
    ];
    let len_before = msgs.len();
    sanitize_messages(&mut msgs);
    assert_eq!(msgs.len(), len_before);
}

// ── 4. check_message_integrity ─────────────────────────────────

#[test]
fn test_integrity_clean() {
    let msgs = vec![
        Message::user("hi"),
        assistant_text("hello"),
        Message::user("go"),
    ];
    let check = check_message_integrity(&msgs);
    assert!(check.is_clean());
    assert!(!check.has_issues());
}

#[test]
fn test_integrity_detects_orphaned_tool_result() {
    let msgs = vec![
        Message::user("hi"),
        assistant_text("hello"),
        // ToolResult referencing a tool_use_id that never appeared
        Message::tool_results(vec![("nonexistent".into(), "data".into(), false)]),
    ];
    let check = check_message_integrity(&msgs);
    assert!(check.has_issues());
    let found = check
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::OrphanedToolResult { .. }));
    assert!(found, "expected OrphanedToolResult issue");
}

#[test]
fn test_integrity_detects_orphaned_tool_use() {
    let msgs = vec![
        Message::user("go"),
        assistant_with_tools(&["tool_x"]),
        // No matching ToolResult — ends with orphaned tool call
    ];
    let check = check_message_integrity(&msgs);
    assert!(check.has_issues());
    let found = check
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::OrphanedToolUse { .. }));
    assert!(found, "expected OrphanedToolUse issue");
}

#[test]
fn test_integrity_detects_role_order() {
    let msgs = vec![
        Message::user("first"),
        Message::user("second"), // back-to-back User — bad
        assistant_text("reply"),
    ];
    let check = check_message_integrity(&msgs);
    assert!(check.has_issues());
    let found = check
        .issues
        .iter()
        .any(|i| matches!(i, IntegrityIssue::RoleOrder { .. }));
    assert!(found, "expected RoleOrder issue");
}

// ── 5. deep_clean_messages ─────────────────────────────────────

#[test]
fn test_deep_clean_removes_mid_orphaned_tool_result() {
    let mut msgs: Vec<Message> = vec![
        Message::user("start"),
        assistant_text("reply"),
        // Orphaned ToolResult in the middle (no matching ToolUse)
        Message::tool_results(vec![("orphan".into(), "x".into(), false)]),
        assistant_text("final"),
        Message::user("end"),
    ];
    let removed = deep_clean_messages(&mut msgs);
    assert!(removed > 0, "should have removed at least one message");
    // The orphaned ToolResult message should be gone
    assert!(msgs
        .iter()
        .all(|m| !m.has_tool_results()
            || m.tool_result_ids().into_iter().any(|id| id != "orphan")));
}

#[test]
fn test_deep_clean_fixes_role_order_with_placeholder() {
    let mut msgs = vec![
        Message::user("first"),
        Message::user("second"), // back-to-back User
        assistant_text("reply"),
    ];
    let _removed = deep_clean_messages(&mut msgs);
    // Verify roles now alternate
    for pair in msgs.windows(2) {
        assert_ne!(pair[0].role, pair[1].role);
    }
}

#[test]
fn test_deep_clean_handles_trailing_orphaned_tool_use() {
    let mut msgs = vec![Message::user("go"), assistant_with_tools(&["tool_1"])];
    let removed = deep_clean_messages(&mut msgs);
    assert!(removed > 0);
    // Trailing orphaned ToolUse assistant should have been removed
    assert!(
        msgs.last().unwrap().role == Role::User,
        "last message should be the user message"
    );
}

#[test]
fn test_deep_clean_clean_messages_noop() {
    let mut msgs = vec![
        Message::user("hi"),
        assistant_text("hello"),
        Message::user("go"),
    ];
    let removed = deep_clean_messages(&mut msgs);
    assert_eq!(removed, 0);
    assert_eq!(msgs.len(), 3);
}

// ── 6. IntegrityCheck helpers ──────────────────────────────────

#[test]
fn test_integrity_check_default_is_clean() {
    let check = IntegrityCheck::default();
    assert!(check.is_clean());
    assert!(!check.has_issues());
    assert!(check.issues.is_empty());
}

#[test]
fn test_integrity_check_with_issues() {
    let check = IntegrityCheck {
        issues: vec![IntegrityIssue::RoleOrder {
            msg_index: 1,
            role: "User".to_string(),
        }],
    };
    assert!(!check.is_clean());
    assert!(check.has_issues());
}
