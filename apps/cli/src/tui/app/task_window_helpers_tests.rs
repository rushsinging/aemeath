use super::*;

pub(crate) fn make_task_with_ts(id: &str, subject: &str, status: TaskStatus, ts: u64) -> Task {
    Task {
        id: id.to_string(),
        subject: subject.to_string(),
        description: String::new(),
        status,
        active_form: None,
        owner: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        priority: ::runtime::api::core::task::TaskPriority::Normal,
        progress: 0,
        progress_message: None,
        created_at: ts,
        updated_at: ts,
        session_id: None,
        tags: Vec::new(),
        batch: 0,
    }
}

pub(crate) fn make_task(id: &str, subject: &str, status: TaskStatus) -> Task {
    make_task_with_ts(id, subject, status, id.parse::<u64>().unwrap_or(100))
}

/// Build a display map from a slice of tasks (sorted by global id ascending).
/// Excludes deleted tasks to match TaskStore.get_batch_display_map behavior.
pub(crate) fn make_display_map(tasks: &[Task]) -> std::collections::HashMap<String, usize> {
    let mut ids: Vec<&str> = tasks
        .iter()
        .filter(|t| t.status != ::runtime::api::core::task::TaskStatus::Deleted)
        .map(|t| t.id.as_str())
        .collect();
    ids.sort_by_key(|id| id.parse::<u64>().unwrap_or(u64::MAX));
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id.to_string(), i + 1))
        .collect()
}
