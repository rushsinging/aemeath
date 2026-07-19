use super::*;
use share::message::{ContentBlock, Message, Role};

fn assistant_tool(name: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: "tool".to_string(),
            name: name.to_string(),
            input: serde_json::json!({}),
        }],
        metadata: None,
    }
}

#[test]
fn task_management_call_updates_observed_turn() {
    let mut state = TaskReminderState::new();
    state.update_from_messages(7, &[assistant_tool("TaskUpdate")]);
    assert_eq!(state.last_task_management_turn(), 7);
}

#[test]
fn unrelated_tool_keeps_previous_turn() {
    let mut state = TaskReminderState::new();
    state.update_from_messages(7, &[assistant_tool("Read")]);
    assert_eq!(state.last_task_management_turn(), 0);
}
