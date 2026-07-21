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
fn every_task_mutation_tool_updates_observed_turn() {
    for tool_name in [
        "TaskCreate",
        "TaskUpdate",
        "TaskStop",
        "TaskListCreate",
        "TaskListComplete",
    ] {
        let mut state = TaskReminderState::new();
        state.update_from_messages(7, &[assistant_tool(tool_name)]);
        assert_eq!(state.last_task_management_turn(), 7, "{tool_name}");
    }
}

#[test]
fn latest_assistant_task_activity_is_preserved_across_unrelated_turns() {
    let mut state = TaskReminderState::new();
    state.update_from_messages(7, &[assistant_tool("TaskUpdate")]);
    state.update_from_messages(8, &[assistant_tool("Read")]);
    assert_eq!(state.last_task_management_turn(), 7);
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
