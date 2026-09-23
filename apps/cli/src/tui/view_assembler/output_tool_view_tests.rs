use super::display_text_for_tool_result;

#[test]
fn test_tool_result_display_text_expands_tab_via_shared_normalize() {
    let content = serde_json::json!({ "display": "col1\tcol2" });
    let text = display_text_for_tool_result(Some("Bash"), "fallback", &content);
    assert_eq!(text, "col1    col2");
}

#[test]
fn test_tool_result_display_text_replaces_escape_via_shared_normalize() {
    // 共享策略增强：ESC 阻断（原 expand_tabs 会原样保留）。
    let content = serde_json::json!({ "display": "a\u{1b}[31m" });
    let text = display_text_for_tool_result(Some("Bash"), "fallback", &content);
    assert_eq!(text, "a\u{fffd}[31m");
}

#[test]
fn test_worktree_branch_message_normalized_via_shared_fn() {
    // 守护提前 return 分支（message+branch 拼接路径）也走共享归一化。
    let content = serde_json::json!({ "message": "a\tb", "branch": "feat/x" });
    let text = display_text_for_tool_result(Some("EnterWorktree"), "fallback", &content);
    assert_eq!(text, "a    b\n当前分支：feat/x");
}
