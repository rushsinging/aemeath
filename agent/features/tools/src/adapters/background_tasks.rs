//! `BackgroundTasks` 工具（#252）：后台任务查询与停止。

use crate::domain::background_task_port::BackgroundTaskAccess;
use crate::domain::types::background_tasks::{
    BackgroundTasksAction, BackgroundTasksInput, BackgroundTasksResult,
};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct BackgroundTasksTool {
    pub access: Arc<dyn BackgroundTaskAccess>,
}

#[cfg(test)]
#[path = "background_tasks_tests.rs"]
mod tests;

const DEFAULT_LOG_MAX_BYTES: usize = 4096;

#[async_trait]
impl TypedTool for BackgroundTasksTool {
    type Output = BackgroundTasksResult;
    fn name(&self) -> &str {
        "BackgroundTasks"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_tasks("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_tasks(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTasksInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTasksResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<BackgroundTasksResult> {
        let args: BackgroundTasksInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        match args.action {
            BackgroundTasksAction::List => {
                let tasks = self.access.list_tasks();
                let text = if tasks.is_empty() {
                    "No background tasks.".to_string()
                } else {
                    tasks
                        .iter()
                        .map(|summary| {
                            format!(
                                "{} [{}] {} ({}ms)",
                                summary.task_id,
                                summary.state,
                                summary.tool_name,
                                summary.duration_ms.unwrap_or(0)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                TypedToolResult::success(
                    text,
                    BackgroundTasksResult {
                        action: Some("list".to_string()),
                        tasks,
                        ..Default::default()
                    },
                )
            }
            BackgroundTasksAction::Status => {
                let Some(task_id) = args.task_id.as_deref() else {
                    return TypedToolResult::error("status action requires task_id");
                };
                match self.access.task_status(task_id) {
                    Some(detail) => TypedToolResult::success(
                        format!(
                            "{} [{}] {} — deadline in {:?}ms, log bytes {}",
                            detail.summary.task_id,
                            detail.summary.state,
                            detail.summary.summary,
                            detail.deadline_remaining_ms,
                            detail.total_written_bytes
                        ),
                        BackgroundTasksResult {
                            action: Some("status".to_string()),
                            detail: Some(detail),
                            ..Default::default()
                        },
                    ),
                    None => TypedToolResult::error(format!("Task not found: {task_id}")),
                }
            }
            BackgroundTasksAction::Logs => {
                let Some(task_id) = args.task_id.as_deref() else {
                    return TypedToolResult::error("logs action requires task_id");
                };
                let max_bytes = args
                    .max_bytes
                    .map(|value| value as usize)
                    .unwrap_or(DEFAULT_LOG_MAX_BYTES);
                match self.access.read_task_log(task_id, args.cursor, max_bytes) {
                    Some(log) => TypedToolResult::success(
                        log.text.clone(),
                        BackgroundTasksResult {
                            action: Some("logs".to_string()),
                            log: Some(log),
                            ..Default::default()
                        },
                    ),
                    None => TypedToolResult::error(format!("Task not found: {task_id}")),
                }
            }
            BackgroundTasksAction::Stop => {
                let Some(task_id) = args.task_id.as_deref() else {
                    return TypedToolResult::error("stop action requires task_id");
                };
                match self.access.stop_task(task_id) {
                    Ok(stop) => TypedToolResult::success(
                        format!("stop requested: {} state={}", task_id, stop.state),
                        BackgroundTasksResult {
                            action: Some("stop".to_string()),
                            stop: Some(stop),
                            ..Default::default()
                        },
                    ),
                    Err(message) => TypedToolResult::error(message),
                }
            }
        }
    }
}
