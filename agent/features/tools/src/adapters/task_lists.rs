use crate::domain::types::task_list::{TaskListMetadata, TaskListStats};
use crate::domain::types::task_lists::{TaskListSummary, TaskListsInput, TaskListsResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{BatchStatus, TaskAccess};

pub struct TaskListsTool {
    pub access: Arc<dyn TaskAccess>,
}

fn status_name(status: BatchStatus) -> String {
    format!("{status:?}").to_ascii_lowercase()
}

#[async_trait]
impl TypedTool for TaskListsTool {
    type Output = TaskListsResult;

    fn name(&self) -> &str {
        "TaskLists"
    }

    fn description(&self) -> &str {
        "List current and historical task lists. Use their IDs with TaskList to inspect a specific list."
    }

    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_lists(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListsInput::data_schema()
    }

    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListsResult::data_schema()
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
    ) -> TypedToolResult<TaskListsResult> {
        let args: TaskListsInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let status = match args.status.as_deref() {
            None => None,
            Some("active") => Some(BatchStatus::Active),
            Some("paused") => Some(BatchStatus::Paused),
            Some("archived") => Some(BatchStatus::Archived),
            Some(value) => {
                return TypedToolResult::error(format!("invalid task list status: {value}"))
            }
        };
        let task_lists = self
            .access
            .list_batch_snapshots()
            .into_iter()
            .filter(|snapshot| status.is_none_or(|status| snapshot.batch().status() == status))
            .map(|snapshot| {
                let batch = snapshot.batch();
                let stats = snapshot.stats();
                TaskListSummary {
                    task_list: TaskListMetadata {
                        id: batch.id().to_string(),
                        summary: batch.summary().unwrap_or_default().to_owned(),
                        status: status_name(batch.status()),
                        created_at: batch.created_at(),
                        last_active_turn: batch.last_active_turn(),
                        silence_turns: batch.silence_turns(),
                    },
                    stats: TaskListStats {
                        total: stats.total,
                        pending: stats.pending,
                        in_progress: stats.in_progress,
                        completed: stats.completed,
                    },
                }
            })
            .collect::<Vec<_>>();
        TypedToolResult::success(
            format!("{} task lists", task_lists.len()),
            TaskListsResult { task_lists },
        )
    }
}

#[cfg(test)]
#[path = "task_lists_tests.rs"]
mod tests;
