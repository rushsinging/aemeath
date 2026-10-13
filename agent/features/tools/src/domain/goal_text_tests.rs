use super::resolve;
use serde_json::json;

#[test]
fn prefers_nonempty_goal_field() {
    let text = resolve(
        "Grep",
        &json!({"goal": "找 timeout", "pattern": "timeout_secs"}),
    );
    assert_eq!(text, "找 timeout");
}

#[test]
fn synthesizes_grep_without_goal() {
    let text = resolve(
        "Grep",
        &json!({"pattern": "timeout_secs", "path": "apps/cli"}),
    );
    assert_eq!(text, "/timeout_secs/, path=apps/cli");
}

#[test]
fn bash_falls_back_to_command() {
    let text = resolve("Bash", &json!({"command": "ping -c 1 127.0.0.1"}));
    assert_eq!(text, "ping -c 1 127.0.0.1");
}

#[test]
fn agent_uses_description_when_no_goal() {
    let text = resolve(
        "Agent",
        &json!({"description": "只读探查", "prompt": "长正文"}),
    );
    assert_eq!(text, "只读探查");
}

#[test]
fn blank_goal_falls_through_to_payload() {
    let text = resolve("Read", &json!({"goal": "   ", "file_path": "Cargo.toml"}));
    assert_eq!(text, "Cargo.toml");
}

#[test]
fn tool_call_goal_text_reads_self_input() {
    use crate::domain::published_language::ToolCall;
    use crate::domain::ExecutionScope;
    let scope = ExecutionScope::builder(
        "run-goal",
        share::session_types::WorkspaceId::from("ws"),
        std::path::PathBuf::from("/tmp"),
    )
    .build();
    let call = ToolCall::new(
        "Bash",
        serde_json::json!({"goal": "跑 ping 验收", "command": "ping -c 1 127.0.0.1"}),
        scope,
    );
    assert_eq!(call.goal_text(), "跑 ping 验收");
}
