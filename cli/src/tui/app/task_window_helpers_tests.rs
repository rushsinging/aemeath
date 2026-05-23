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
        priority: aemeath_core::task::TaskPriority::Normal,
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
