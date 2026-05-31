use crate::business::compact::restore::assemble::assemble_compacted;
use crate::business::compact::restore::assemble::fix_role_alternation;
use crate::business::compact::restore::sanitize_pairs::sanitize_tool_pairs;
use share::message::{ContentBlock, Message, Role};

// helper
fn text_msg(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
    }
}

// ── fix_role_alternation ────────────────────────────────────

#[test]
fn fix_role_alternation_already_alternating_unchanged() {
    let mut msgs = vec![
        text_msg(Role::User, "u1"),
        text_msg(Role::Assistant, "a1"),
        text_msg(Role::User, "u2"),
    ];
    fix_role_alternation(&mut msgs);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(msgs[1].role, Role::Assistant);
    assert_eq!(msgs[2].role, Role::User);
}

#[test]
fn fix_role_alternation_two_consecutive_same_role_inserts_placeholder() {
    let mut msgs = vec![
        text_msg(Role::User, "u1"),
        text_msg(Role::User, "u2"),
        text_msg(Role::Assistant, "a1"),
    ];
    fix_role_alternation(&mut msgs);
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(msgs[1].role, Role::Assistant); // placeholder inserted
    assert_eq!(msgs[2].role, Role::User);
    assert_eq!(msgs[3].role, Role::Assistant);
}

#[test]
fn fix_role_alternation_three_consecutive_same_role_inserts_two_placeholders() {
    let mut msgs = vec![
        text_msg(Role::Assistant, "a1"),
        text_msg(Role::Assistant, "a2"),
        text_msg(Role::Assistant, "a3"),
    ];
    fix_role_alternation(&mut msgs);
    assert_eq!(msgs.len(), 5);
    assert_eq!(msgs[0].role, Role::Assistant);
    assert_eq!(msgs[1].role, Role::User); // placeholder
    assert_eq!(msgs[2].role, Role::Assistant);
    assert_eq!(msgs[3].role, Role::User); // placeholder
    assert_eq!(msgs[4].role, Role::Assistant);
}

#[test]
fn fix_role_alternation_empty_and_single_unchanged() {
    let mut empty: Vec<Message> = vec![];
    fix_role_alternation(&mut empty);
    assert!(empty.is_empty());

    let mut single = vec![text_msg(Role::User, "u1")];
    fix_role_alternation(&mut single);
    assert_eq!(single.len(), 1);
}

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
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: serde_json::Value::String("ok".to_string()),
                is_error: false,
            }],
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
        }],
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
        },
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String("result1".to_string()),
                is_error: false,
            }],
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

// ── assemble_compacted ──────────────────────────────────────

#[test]
fn assemble_compacted_basic_summary_and_recent() {
    let recent = vec![
        text_msg(Role::User, "hello"),
        text_msg(Role::Assistant, "hi"),
    ];
    let (compacted, did_compact) = assemble_compacted("summary text".to_string(), &recent, 5);
    assert!(did_compact);

    // First message is User with summary
    assert_eq!(compacted[0].role, Role::User);
    match &compacted[0].content[0] {
        ContentBlock::Text { text } => {
            assert!(text.contains("summary text"));
            assert!(text.contains("5 earlier messages"));
        }
        _ => panic!("expected Text block"),
    }

    // Second message is Assistant ack
    assert_eq!(compacted[1].role, Role::Assistant);

    // Then the recent messages
    assert_eq!(compacted[2].role, Role::User);
    assert_eq!(compacted[3].role, Role::Assistant);
}

#[test]
fn assemble_compacted_role_alternation_enforced() {
    // Both recent messages are User — should get placeholders inserted
    let recent = vec![text_msg(Role::User, "q1"), text_msg(Role::User, "q2")];
    let (compacted, _) = assemble_compacted("sum".to_string(), &recent, 3);

    for w in compacted.windows(2) {
        assert_ne!(
            w[0].role, w[1].role,
            "consecutive messages must alternate roles"
        );
    }
}
