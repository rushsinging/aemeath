use crate::domain::types::task_block_by::{TaskBlockByInput, TaskBlockByResult};
use crate::domain::{CommittedTaskChange, ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use task::{TaskAccess, TaskId};

pub struct TaskBlockByTool {
    pub access: Arc<dyn TaskAccess>,
}

fn current_task_id(access: &dyn TaskAccess, value: &str, field: &str) -> Result<TaskId, String> {
    let seq = TaskId::parse_tool_input(value)
        .map(TaskId::get)
        .map_err(|_| format!("{field} must contain non-zero decimal task ID sequences: {value}"))?;
    access
        .current_task_by_seq(seq)
        .map(|task| task.id())
        .ok_or_else(|| format!("Task not found in current task list: {value}"))
}

#[async_trait]
impl TypedTool for TaskBlockByTool {
    type Output = TaskBlockByResult;

    fn name(&self) -> &str {
        "TaskBlockBy"
    }

    fn description(&self) -> &str {
        "Replace all blocking dependencies of a task. Pass id and the complete block_by_ids list; an empty list clears dependencies."
    }

    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_block_by(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskBlockByInput::data_schema()
    }

    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskBlockByResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskBlockByResult> {
        let args: TaskBlockByInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let id = match current_task_id(self.access.as_ref(), &args.id, "id") {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };
        let mut seen = HashSet::with_capacity(args.block_by_ids.len());
        let mut dependencies = Vec::with_capacity(args.block_by_ids.len());
        for value in &args.block_by_ids {
            let dependency = match current_task_id(self.access.as_ref(), value, "block_by_ids") {
                Ok(id) => id,
                Err(error) => return TypedToolResult::error(error),
            };
            if !seen.insert(dependency) {
                return TypedToolResult::error(format!(
                    "block_by_ids contains duplicate task ID: {value}"
                ));
            }
            dependencies.push(dependency);
        }

        let command_result = match self.access.replace_dependencies(
            id,
            dependencies,
            chrono::Utc::now().timestamp_millis() as u64,
        ) {
            Ok(result) => result,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let task_change = CommittedTaskChange::from_command_result(&command_result);
        let updated = command_result.value;
        let blocked_by_ids = updated
            .blocked_by()
            .iter()
            .filter_map(|dependency_id| {
                self.access
                    .get(*dependency_id)
                    .map(|task| task.seq().to_string())
            })
            .collect::<Vec<_>>();
        let task_id = updated.seq().to_string();
        TypedToolResult::success(
            format!(
                "Task #{} blocking dependencies replaced: {}",
                task_id,
                if blocked_by_ids.is_empty() {
                    "none".to_string()
                } else {
                    blocked_by_ids
                        .iter()
                        .map(|id| format!("#{id}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ),
            TaskBlockByResult {
                task_id,
                subject: updated.subject().to_owned(),
                blocked_by_ids,
            },
        )
        .with_task_change(task_change)
    }
}

#[cfg(test)]
#[path = "task_block_by_tests.rs"]
mod tests;
