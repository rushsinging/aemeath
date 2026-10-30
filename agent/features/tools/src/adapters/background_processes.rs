//! 后台进程工具族（#252 / #1931）：list / status / logs / stop / watch。

use crate::adapters::constants::{
    WATCH_RESULT_HEADER_RESERVE_CHARS, WATCH_RESULT_TEXT_BUDGET_CHARS,
};
use crate::domain::background_process_port::{
    wait_task, BackgroundProcessAccess, BackgroundProcessAccessSource,
};
use crate::domain::constants::WATCH_TOOL_TIMEOUT_SECS;
use crate::domain::types::background_processes::{
    resolve_watch_wait_secs, BackgroundProcessDetailData, BackgroundProcessListInput,
    BackgroundProcessListResult, BackgroundProcessLogData, BackgroundProcessLogsInput,
    BackgroundProcessLogsResult, BackgroundProcessStatusInput, BackgroundProcessStatusResult,
    BackgroundProcessStopInput, BackgroundProcessStopResult, BackgroundProcessSummaryData,
    BackgroundProcessWatchInput, BackgroundProcessWatchOutcome, BackgroundProcessWatchResult,
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
                let text = render_log_text(&log);
                TypedToolResult::success(text, BackgroundProcessLogsResult { log })
            }
            None => TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        }
    }
}

/// 日志读结果自述：窗口字节区间 + 未包含字节数 + 全量日志文件路径。
///
/// 缺省游标的尾部窗口只含最后 `max_bytes` 字节；不点明会让读方把
/// 「开头是半截内容」误判为文件损坏或落盘截断。日志文件是全量真相，
/// 因此**MUST** 显式给出路径并提示直接读文件，而不是反复调本工具。
fn render_log_text(log: &BackgroundProcessLogData) -> String {
    let mut header = format!(
        "[log bytes {}..{} of {}]",
        log.start_offset, log.cursor, log.total_written
    );
    if log.start_offset > 0 {
        header.push_str(&format!(
            "\n{} earlier bytes are NOT included in this text (tail window only)",
            log.start_offset
        ));
    }
    if let Some(path) = log.log_file.as_deref() {
        header.push_str(&format!(
            "\nfull log file: {path} (complete and not truncated — read that file directly for the whole log)"
        ));
    }
    if log.start_offset > 0 {
        header.push_str("\npass cursor=0 to read from the beginning of this log");
    }
    if log.text.is_empty() {
        header
    } else {
        format!("{header}\n{}", log.text)
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
    fn get_timeout_secs(&self, requested: Option<u64>, _backgrounding_enabled: bool) -> u64 {
        // #1946：框架硬上限固定 300，且本工具永不转后台，后台档
        // （24h）对 Watch 不生效；入参 timeout 只能缩短内部 wait。
        crate::domain::timeout_policy::get_timeout_secs(
            requested,
            WATCH_TOOL_TIMEOUT_SECS,
            WATCH_TOOL_TIMEOUT_SECS,
            false,
        )
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
        // `timeout` 是框架级通用参数：框架已解析成 effective 值（含 300s 硬上限）
        // 写入 scope，执行前从入参摘除；这里只读 scope，**NEVER** 读 input。
        let timeout = Duration::from_secs(resolve_watch_wait_secs(
            ctx.scope().effective_timeout_secs(),
        ));
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

/// 终态附带 log tail：按 Watch 源头硬预算裁剪（日志本已落盘，防 materializer 二次落盘）。
fn attach_watch_log_tail(
    access: &dyn BackgroundProcessAccess,
    task_id: &str,
    result: &mut BackgroundProcessWatchResult,
) {
    let body_budget =
        WATCH_RESULT_TEXT_BUDGET_CHARS.saturating_sub(WATCH_RESULT_HEADER_RESERVE_CHARS);
    let Some(log) = access.read_task_log(task_id, None, body_budget) else {
        return;
    };
    result.log_start_offset = log.start_offset;
    result.log_file = log.log_file.clone();
    result.log_tail = clip_to_char_budget(&log.text, body_budget);
}

fn watch_text(result: &BackgroundProcessWatchResult) -> String {
    let outcome = match result.outcome {
        BackgroundProcessWatchOutcome::Terminal => "terminal",
        BackgroundProcessWatchOutcome::StillRunning => "still_running",
        BackgroundProcessWatchOutcome::Cancelled => "cancelled",
    };
    let mut header = format!(
        "{} outcome={} [{}] {} — log bytes {}",
        result.detail.summary.task_id,
        outcome,
        result.detail.summary.state,
        result.detail.summary.goal,
        result.detail.total_written_bytes
    );
    if result.log_start_offset > 0 {
        header.push_str(&format!(
            " (tail window from byte {})",
            result.log_start_offset
        ));
    }
    if let Some(path) = result.log_file.as_deref() {
        header.push_str(&format!(
            "\nfull log file: {path} (complete and not truncated — read that file directly)"
        ));
    }
    if result.log_tail.is_empty() {
        return header;
    }
    let remaining =
        WATCH_RESULT_TEXT_BUDGET_CHARS.saturating_sub(header.chars().count().saturating_add(1));
    let body = clip_to_char_budget(&result.log_tail, remaining);
    format!("{header}\n{body}")
}

fn clip_to_char_budget(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let marker = "\n…(truncated)";
    let marker_chars = marker.chars().count();
    let keep = budget.saturating_sub(marker_chars);
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= keep {
            break;
        }
        out.push(ch);
    }
    if let Some((head, _)) = out.rsplit_once('\n') {
        out = head.to_string();
    }
    out.push_str(marker);
    // 若仍超预算（极端短 budget），硬裁到 budget。
    if out.chars().count() > budget {
        out = out.chars().take(budget).collect();
    }
    out
}
