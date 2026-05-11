use super::*;

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
    assert!(
        msgs.iter()
            .all(|m| !m.has_tool_results()
                || m.tool_result_ids().into_iter().any(|id| id != "orphan"))
    );
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
