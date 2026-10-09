//! 后台进程工具族（#252）：list / status / logs / stop 四个独立 tool。

use crate::domain::background_process_port::BackgroundProcessAccessSource;
use crate::domain::types::background_processes::{
    BackgroundProcessDetailData, BackgroundProcessListInput, BackgroundProcessListResult,
    BackgroundProcessLogsInput, BackgroundProcessLogsResult, BackgroundProcessStatusInput,
    BackgroundProcessStatusResult, BackgroundProcessStopInput, BackgroundProcessStopResult,
    BackgroundProcessSummaryData,
};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

#[cfg(test)]
#[path = "background_processes_tests.rs"]
mod tests;

/// `BackgroundProcessList`：列出活动与近期后台进程。
pub struct BackgroundProcessListTool {
    pub source: Arc<dyn BackgroundProcessAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundProcessListTool {
    type Output = BackgroundProcessListResult;
    fn name(&self) -> &str {
        "BackgroundProcessList"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_process_list("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_process_list(
            lang,
        ))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessListInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessListResult::data_schema()
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
    ) -> TypedToolResult<BackgroundProcessListResult> {
        if let Err(error) = serde_json::from_value::<BackgroundProcessListInput>(input) {
            return TypedToolResult::error(format!("invalid input: {error}"));
        }
        let tasks = self.source.current().list_tasks();
        let text = if tasks.is_empty() {
            "No background processes.".to_string()
        } else {
            tasks
                .iter()
                .map(|summary: &BackgroundProcessSummaryData| {
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
        TypedToolResult::success(text, BackgroundProcessListResult { tasks })
    }
}

/// `BackgroundProcessStatus`：单任务详情。
pub struct BackgroundProcessStatusTool {
    pub source: Arc<dyn BackgroundProcessAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundProcessStatusTool {
    type Output = BackgroundProcessStatusResult;
    fn name(&self) -> &str {
        "BackgroundProcessStatus"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_process_status("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_process_status(
            lang,
        ))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessStatusInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessStatusResult::data_schema()
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
    ) -> TypedToolResult<BackgroundProcessStatusResult> {
        let args: BackgroundProcessStatusInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        match self.source.current().task_status(&args.task_id) {
            Some(detail) => TypedToolResult::success(
                status_text(&detail),
                BackgroundProcessStatusResult { detail },
            ),
            None => TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        }
    }
}

fn status_text(detail: &BackgroundProcessDetailData) -> String {
    format!(
        "{} [{}] {} — deadline in {:?}ms, log bytes {}",
        detail.summary.task_id,
        detail.summary.state,
        detail.summary.summary,
        detail.deadline_remaining_ms,
        detail.total_written_bytes
    )
}

/// `BackgroundProcessLogs`：任务日志读取（增量游标）。
pub struct BackgroundProcessLogsTool {
    pub source: Arc<dyn BackgroundProcessAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundProcessLogsTool {
    type Output = BackgroundProcessLogsResult;
    fn name(&self) -> &str {
        "BackgroundProcessLogs"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_process_logs("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_process_logs(
            lang,
        ))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessLogsInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessLogsResult::data_schema()
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
    ) -> TypedToolResult<BackgroundProcessLogsResult> {
        let args: BackgroundProcessLogsInput = match serde_json::from_value(input) {
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
                TypedToolResult::success(log.text.clone(), BackgroundProcessLogsResult { log })
            }
            None => TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        }
    }
}

/// `BackgroundProcessStop`：请求停止（发 cancel 信号，真实终态由执行体收口）。
pub struct BackgroundProcessStopTool {
    pub source: Arc<dyn BackgroundProcessAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundProcessStopTool {
    type Output = BackgroundProcessStopResult;
    fn name(&self) -> &str {
        "BackgroundProcessStop"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_process_stop("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_process_stop(
            lang,
        ))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessStopInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessStopResult::data_schema()
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
    ) -> TypedToolResult<BackgroundProcessStopResult> {
        let args: BackgroundProcessStopInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        match self.source.current().stop_task(&args.task_id) {
            Ok(stop) => TypedToolResult::success(
                format!("stop requested: {} state={}", args.task_id, stop.state),
                BackgroundProcessStopResult {
                    task_id: args.task_id,
                    stop,
                },
            ),
            Err(message) => TypedToolResult::error(message),
        }
    }
}
