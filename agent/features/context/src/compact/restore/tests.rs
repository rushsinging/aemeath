use crate::compact::restore::sanitize_pairs::sanitize_tool_pairs;
use share::message::{ContentBlock, Message, Role};

// ── sanitize_tool_pairs ─────────────────────────────────────

#[test]
fn sanitize_tool_pairs_complete_pair_untouched() {
    let mut msgs = vec![
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({}),
            }],
            metadata: None,
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: serde_json::Value::String("ok".to_string()),
                is_error: false,
                text: None,
            }],
            metadata: None,
        },
    ];
    sanitize_tool_pairs(&mut msgs);
    assert_eq!(msgs.len(), 2);
}

#[test]
fn sanitize_tool_pairs_orphan_tool_result_removed() {
    let mut msgs = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "nonexistent".to_string(),
            content: serde_json::Value::String("orphan".to_string()),
            is_error: false,
            text: None,
        }],
        metadata: None,
    }];
    sanitize_tool_pairs(&mut msgs);
    // orphan removed, then empty message removed
    assert!(msgs.is_empty());
}

#[test]
fn sanitize_tool_pairs_missing_tool_result_gets_placeholder() {
    let mut msgs = vec![Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "tool_1".to_string(),
            name: "Bash".to_string(),
            input: serde_json::json!({}),
        }],
        metadata: None,
    }];
    sanitize_tool_pairs(&mut msgs);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].role, Role::User);
    // Should contain a placeholder ToolResult for tool_1
    let has_placeholder = msgs[1].content.iter().any(|b| {
        matches!(
            b,
            ContentBlock::ToolResult { tool_use_id, .. }
            if tool_use_id == "tool_1"
        )
    });
    assert!(has_placeholder);
}

#[test]
fn sanitize_tool_pairs_mixed_partial_results() {
    let mut msgs = vec![
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolUse {
                    id: "t2".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({}),
                },
            ],
            metadata: None,
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String("result1".to_string()),
                is_error: false,
                text: None,
            }],
            metadata: None,
        },
    ];
    sanitize_tool_pairs(&mut msgs);
    // t1 has result, t2 should get a placeholder
    let user_msg = &msgs[1];
    let t2_present = user_msg.content.iter().any(|b| {
        matches!(
            b,
            ContentBlock::ToolResult { tool_use_id, .. }
            if tool_use_id == "t2"
        )
    });
    assert!(t2_present, "placeholder for t2 should be present");
}
