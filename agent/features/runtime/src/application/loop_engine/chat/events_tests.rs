use tools::{CommittedTaskChange, ToolOutcome};

fn tool_outcome_has_committed_task_change(outcome: &ToolOutcome) -> bool {
    outcome.task_change.is_some()
}

#[test]
fn task_refresh_gating_uses_committed_change_not_tool_name() {
    let outcome = ToolOutcome::new("Task #1 updated", serde_json::Value::Null, Vec::new());

    assert!(!tool_outcome_has_committed_task_change(&outcome));
}

#[test]
fn committed_change_requests_task_refresh_independent_of_tool_name() {
    let store = task::TaskStore::new();
    let result = task::TaskAccess::create_batch(
        &store,
        task::BatchCreateSpec::try_new("batch".to_owned()).unwrap(),
        1,
    )
    .unwrap();
    let change = CommittedTaskChange::from_command_result(&result).unwrap();
    let outcome = ToolOutcome::new("changed", serde_json::Value::Null, Vec::new())
        .with_task_change(Some(change));

    assert!(tool_outcome_has_committed_task_change(&outcome));
}
