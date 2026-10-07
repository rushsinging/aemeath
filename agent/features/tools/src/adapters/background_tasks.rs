//! 后台任务工具族（#252）：list / status / logs / stop 四个独立 tool。

use crate::domain::background_task_port::BackgroundTaskAccessSource;
use crate::domain::types::background_tasks::{
    BackgroundTaskDetailData, BackgroundTaskListInput, BackgroundTaskListResult,
    BackgroundTaskLogsInput, BackgroundTaskLogsResult, BackgroundTaskStatusInput,
    BackgroundTaskStatusResult, BackgroundTaskStopInput, BackgroundTaskStopResult,
    BackgroundTaskSummaryData,
};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

#[cfg(test)]
#[path = "background_tasks_tests.rs"]
mod tests;

/// `BackgroundTaskList`：列出活动与近期后台任务。
pub struct BackgroundTaskListTool {
    pub source: Arc<dyn BackgroundTaskAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundTaskListTool {
    type Output = BackgroundTaskListResult;
    fn name(&self) -> &str {
        "BackgroundTaskList"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_task_list("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_task_list(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskListInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskListResult::data_schema()
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
    ) -> TypedToolResult<BackgroundTaskListResult> {
        if let Err(error) = serde_json::from_value::<BackgroundTaskListInput>(input) {
            return TypedToolResult::error(format!("invalid input: {error}"));
        }
        let tasks = self.source.current().list_tasks();
        let text = if tasks.is_empty() {
            "No background tasks.".to_string()
        } else {
            tasks
                .iter()
                .map(|summary: &BackgroundTaskSummaryData| {
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
        TypedToolResult::success(text, BackgroundTaskListResult { tasks })
    }
}

/// `BackgroundTaskStatus`：单任务详情。
pub struct BackgroundTaskStatusTool {
    pub source: Arc<dyn BackgroundTaskAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundTaskStatusTool {
    type Output = BackgroundTaskStatusResult;
    fn name(&self) -> &str {
        "BackgroundTaskStatus"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_task_status("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_task_status(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskStatusInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskStatusResult::data_schema()
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
    ) -> TypedToolResult<BackgroundTaskStatusResult> {
        let args: BackgroundTaskStatusInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        match self.source.current().task_status(&args.task_id) {
            Some(detail) => TypedToolResult::success(
                status_text(&detail),
                BackgroundTaskStatusResult { detail },
            ),
            None => TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        }
    }
}

fn status_text(detail: &BackgroundTaskDetailData) -> String {
    format!(
        "{} [{}] {} — deadline in {:?}ms, log bytes {}",
        detail.summary.task_id,
        detail.summary.state,
        detail.summary.summary,
        detail.deadline_remaining_ms,
        detail.total_written_bytes
    )
}

/// `BackgroundTaskLogs`：任务日志读取（增量游标）。
pub struct BackgroundTaskLogsTool {
    pub source: Arc<dyn BackgroundTaskAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundTaskLogsTool {
    type Output = BackgroundTaskLogsResult;
    fn name(&self) -> &str {
        "BackgroundTaskLogs"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_task_logs("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_task_logs(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskLogsInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskLogsResult::data_schema()
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
    ) -> TypedToolResult<BackgroundTaskLogsResult> {
        let args: BackgroundTaskLogsInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let max_bytes = args
            .max_bytes
            .map(|value| value as usize)
            .unwrap_or(crate::adapters::constants::DEFAULT_BACKGROUND_LOG_MAX_BYTES);
        match self
            .source
            .current()
            .read_task_log(&args.task_id, args.cursor, max_bytes)
        {
            Some(log) => {
                TypedToolResult::success(log.text.clone(), BackgroundTaskLogsResult { log })
            }
            None => TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        }
    }
}

/// `BackgroundTaskStop`：请求停止（发 cancel 信号，真实终态由执行体收口）。
pub struct BackgroundTaskStopTool {
    pub source: Arc<dyn BackgroundTaskAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundTaskStopTool {
    type Output = BackgroundTaskStopResult;
    fn name(&self) -> &str {
        "BackgroundTaskStop"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_task_stop("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_task_stop(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskStopInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundTaskStopResult::data_schema()
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
    ) -> TypedToolResult<BackgroundTaskStopResult> {
        let args: BackgroundTaskStopInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        match self.source.current().stop_task(&args.task_id) {
            Ok(stop) => TypedToolResult::success(
                format!("stop requested: {} state={}", args.task_id, stop.state),
                BackgroundTaskStopResult {
                    task_id: args.task_id,
                    stop,
                },
            ),
            Err(message) => TypedToolResult::error(message),
        }
    }
}
