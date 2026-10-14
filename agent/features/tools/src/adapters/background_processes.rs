//! 后台进程工具族（#252 / #1931）：list / status / logs / stop / watch。

use crate::adapters::constants::DEFAULT_BACKGROUND_LOG_MAX_BYTES;
use crate::domain::background_process_port::{
    wait_task, BackgroundProcessAccess, BackgroundProcessAccessSource,
};
use crate::domain::constants::WATCH_TOOL_TIMEOUT_SECS;
use crate::domain::types::background_processes::{
    resolve_watch_wait_secs, BackgroundProcessDetailData, BackgroundProcessListInput,
    BackgroundProcessListResult, BackgroundProcessLogsInput, BackgroundProcessLogsResult,
    BackgroundProcessStatusInput, BackgroundProcessStatusResult, BackgroundProcessStopInput,
    BackgroundProcessStopResult, BackgroundProcessSummaryData, BackgroundProcessWatchInput,
    BackgroundProcessWatchOutcome, BackgroundProcessWatchResult,
};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

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
                        "{} [{}] {} · {} ({}ms)",
                        summary.task_id,
                        summary.state,
                        summary.tool_name,
                        summary.goal,
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
        detail.summary.goal,
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

/// `BackgroundProcessWatch`：前台短等任务终态或超时（#1931）。
///
/// **MUST** 由执行监督器禁止转后台；完成通知 / reminder 不因本工具而抑制。
pub struct BackgroundProcessWatchTool {
    pub source: Arc<dyn BackgroundProcessAccessSource>,
}

#[async_trait]
impl TypedTool for BackgroundProcessWatchTool {
    type Output = BackgroundProcessWatchResult;
    fn name(&self) -> &str {
        "BackgroundProcessWatch"
    }
    fn description(&self) -> &str {
        share::i18n::tools::background::background_process_watch("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::background::background_process_watch(
            lang,
        ))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessWatchInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BackgroundProcessWatchResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn timeout_secs(&self) -> u64 {
        // #1946：框架硬上限固定 300；统一入参 timeout 只缩短内部 wait，
        // NEVER 超过此上限，避免 effective deadline 先砍。
        WATCH_TOOL_TIMEOUT_SECS
    }

    fn max_timeout_secs(&self) -> u64 {
        WATCH_TOOL_TIMEOUT_SECS
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<BackgroundProcessWatchResult> {
        let args: BackgroundProcessWatchInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        if args.task_id.is_empty() {
            return TypedToolResult::error("task_id is required".to_string());
        }
        let timeout = Duration::from_secs(resolve_watch_wait_secs(args.timeout));
        let access = self.source.current();
        let progress = ctx.progress_sink();
        match wait_task(
            access.as_ref(),
            &args.task_id,
            timeout,
            ctx.cancellation(),
            progress.as_deref(),
        )
        .await
        {
            Ok(mut result) => {
                attach_watch_log_tail(access.as_ref(), &args.task_id, &mut result);
                TypedToolResult::success(watch_text(&result), result)
            }
            Err(message) => TypedToolResult::error(message),
        }
    }
}

/// 终态附带 log tail：按默认日志字节窗口读取；大结果交给 materializer 兜底。
fn attach_watch_log_tail(
    access: &dyn BackgroundProcessAccess,
    task_id: &str,
    result: &mut BackgroundProcessWatchResult,
) {
    let Some(log) = access.read_task_log(task_id, None, DEFAULT_BACKGROUND_LOG_MAX_BYTES) else {
        return;
    };
    result.log_tail = log.text;
}

fn watch_text(result: &BackgroundProcessWatchResult) -> String {
    let outcome = match result.outcome {
        BackgroundProcessWatchOutcome::Terminal => "terminal",
        BackgroundProcessWatchOutcome::StillRunning => "still_running",
        BackgroundProcessWatchOutcome::Cancelled => "cancelled",
    };
    let header = format!(
        "{} outcome={} [{}] {} — log bytes {}",
        result.detail.summary.task_id,
        outcome,
        result.detail.summary.state,
        result.detail.summary.goal,
        result.detail.total_written_bytes
    );
    if result.log_tail.is_empty() {
        return header;
    }
    format!("{header}\n{}", result.log_tail)
}
