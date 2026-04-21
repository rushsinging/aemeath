use aemeath_core::task::{Task, TaskStatus};

/// 格式化完整任务列表快照
pub fn format_task_snapshot(tasks: &[Task]) -> String {
    let completed = tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();
    let total = tasks.iter().filter(|t| t.status != TaskStatus::Deleted).count();

    if total == 0 {
        return String::new();
    }

    let mut lines = vec![format!("━━━ Tasks: {}/{} completed ━━━", completed, total)];

    for t in tasks {
        if t.status == TaskStatus::Deleted {
            continue;
        }
        let icon = match t.status {
            TaskStatus::Completed => "✓",
            TaskStatus::InProgress => "■",
            TaskStatus::Pending => "□",
            TaskStatus::Deleted => continue,
        };
        let owner = t.owner.as_deref().map(|o| format!(" (@{})", o)).unwrap_or_default();
        let blocked = if !t.blocked_by.is_empty() {
            let deps = t.blocked_by.iter().map(|d| format!("#{d}")).collect::<Vec<_>>().join(", ");
            format!(" (blocked by {})", deps)
        } else {
            String::new()
        };
        lines.push(format!("  {} #{} {}{}{}", icon, t.id, t.subject, owner, blocked));
    }

    lines.join("\n")
}

/// 格式化单条任务状态变更
#[allow(dead_code)]
pub fn format_task_change(task: &Task) -> String {
    match task.status {
        TaskStatus::InProgress => {
            let action = task.active_form.as_deref().unwrap_or("started");
            format!("  ■ #{} {} — {}", task.id, task.subject, action)
        }
        TaskStatus::Completed => {
            format!("  ✓ #{} {} — completed", task.id, task.subject)
        }
        _ => String::new(),
    }
}
