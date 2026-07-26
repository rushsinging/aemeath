use crate::domain::types::task_list::{
    TaskListInput, TaskListMetadata, TaskListResult, TaskListStats,
};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{BatchId, TaskAccess, TaskStatus, TaskView};

pub struct TaskListTool {
    pub access: Arc<dyn TaskAccess>,
}

#[cfg(test)]
#[path = "task_list_tests.rs"]
mod tests;

fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Completed => "completed",
        TaskStatus::Deleted => "deleted",
    }
}

fn format_task_line(
    task: &task::Task,
    seq_by_id: &std::collections::HashMap<task::TaskId, String>,
) -> String {
    let blocked_by = task
        .blocked_by()
        .iter()
        .filter_map(|id| seq_by_id.get(id))
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>();
    let blocked = if blocked_by.is_empty() {
        String::new()
    } else {
        format!(" (blocked by {})", blocked_by.join(", "))
    };
    format!(
        "{}. {} — {}{}",
        task.seq(),
        status_name(task.status()),
        task.subject(),
        blocked
    )
}

#[async_trait]
impl TypedTool for TaskListTool {
    type Output = TaskListResult;
    fn name(&self) -> &str {
        "TaskList"
    }
    fn description(&self) -> &str {
        "List all tasks and their status. Use to discover pending work with no unresolved dependencies."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_list(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskListResult> {
        let args: TaskListInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let status = args.status.as_deref().and_then(|value| match value {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            "deleted" => Some(TaskStatus::Deleted),
            _ => None,
        });
        let target_batch = match args.task_list_id.as_deref() {
            Some(value) => match value.parse::<u64>() {
                Ok(id) if id > 0 => BatchId::new(id),
                _ => return TypedToolResult::error(format!("invalid task list id: {value}")),
            },
            None => match self.access.current_batch() {
                Some(id) => id,
                None => {
                    return TypedToolResult::success(
                        "No current task list",
                        TaskListResult {
                            task_list: None,
                            stats: TaskListStats {
                                total: 0,
                                pending: 0,
                                in_progress: 0,
                                completed: 0,
                            },
                            tasks: Vec::new(),
                        },
                    )
                }
            },
        };
        let snapshot = match self.access.batch_snapshot(target_batch) {
            Some(snapshot) => snapshot,
            None => return TypedToolResult::error(format!("task list {target_batch} not found")),
        };
        let all_tasks = snapshot.tasks();
        let seq_by_id = all_tasks
            .iter()
            .map(|task| (task.id(), task.seq().to_string()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut tasks: Vec<_> = all_tasks.iter().collect();
        if let Some(status) = status {
            tasks.retain(|task| task.status() == status);
        }
        let stats = snapshot.stats();
        let batch = snapshot.batch();
        let mut message = format!(
            "Task list #{}: {}\n{} tasks ({} pending, {} in_progress, {} completed)",
            batch.id(),
            batch.summary().unwrap_or_default(),
            stats.total,
            stats.pending,
            stats.in_progress,
            stats.completed
        );
        for task in &tasks {
            message.push('\n');
            message.push_str(&format_task_line(task, &seq_by_id));
        }
        TypedToolResult::success(
            message,
            TaskListResult {
                task_list: Some(TaskListMetadata {
                    id: batch.id().to_string(),
                    summary: batch.summary().unwrap_or_default().to_owned(),
                    status: format!("{:?}", batch.status()).to_ascii_lowercase(),
                    created_at: batch.created_at(),
                    last_active_turn: batch.last_active_turn(),
                    silence_turns: batch.silence_turns(),
                }),
                stats: TaskListStats {
                    total: stats.total,
                    pending: stats.pending,
                    in_progress: stats.in_progress,
                    completed: stats.completed,
                },
                tasks: tasks
                    .iter()
                    .map(|task| {
                        TaskView::from_task(
                            task,
                            task.blocked_by()
                                .iter()
                                .filter_map(|id| seq_by_id.get(id).cloned())
                                .collect(),
                        )
                    })
                    .collect(),
            },
        )
    }
}
