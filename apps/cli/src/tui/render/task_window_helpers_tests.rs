use sdk::{TaskState, TaskSummary};

pub(crate) fn make_task_with_ts(id: &str, subject: &str, state: TaskState, ts: u64) -> TaskSummary {
    TaskSummary {
        id: id.to_string(),
        subject: subject.to_string(),
        status: match state {
            TaskState::Pending => "pending",
            TaskState::InProgress => "in_progress",
            TaskState::Completed => "completed",
            TaskState::Deleted => "deleted",
        }
        .to_string(),
        state,
        priority: "normal".to_string(),
        owner: None,
        updated_at: ts,
    }
}

pub(crate) fn make_task(id: &str, subject: &str, state: TaskState) -> TaskSummary {
    make_task_with_ts(id, subject, state, id.parse::<u64>().unwrap_or(100))
}

/// Build a display map from a slice of tasks (sorted by global id ascending).
/// Excludes deleted tasks to match TaskStore.get_batch_display_map behavior.
pub(crate) fn make_display_map(tasks: &[TaskSummary]) -> std::collections::HashMap<String, usize> {
    let mut ids: Vec<&str> = tasks
        .iter()
        .filter(|t| t.state != TaskState::Deleted)
        .map(|t| t.id.as_str())
        .collect();
    ids.sort_by_key(|id| id.parse::<u64>().unwrap_or(u64::MAX));
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id.to_string(), i + 1))
        .collect()
}
