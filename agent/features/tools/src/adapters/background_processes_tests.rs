use super::*;
use crate::domain::background_process_port::BackgroundProcessAccess;
use crate::domain::types::background_processes::{
    BackgroundProcessDetailData, BackgroundProcessLogData, BackgroundProcessStopData,
    BackgroundProcessSummaryData,
};
use std::sync::Arc;

struct FakeAccess {
    summaries: Vec<BackgroundProcessSummaryData>,
    log_text: String,
}

impl BackgroundProcessAccess for FakeAccess {
    fn list_tasks(&self) -> Vec<BackgroundProcessSummaryData> {
        self.summaries.clone()
    }

    fn task_status(&self, task_id: &str) -> Option<BackgroundProcessDetailData> {
        let summary = self
            .summaries
            .iter()
            .find(|summary| summary.task_id == task_id)?
            .clone();
        Some(BackgroundProcessDetailData {
            summary,
            deadline_remaining_ms: Some(120_000),
            total_written_bytes: self.log_text.len() as u64,
        })
    }

    fn read_task_log(
        &self,
        task_id: &str,
        cursor: Option<u64>,
        max_bytes: usize,
    ) -> Option<BackgroundProcessLogData> {
        if !self
            .summaries
            .iter()
            .any(|summary| summary.task_id == task_id)
        {
            return None;
        }
        let bytes = self.log_text.as_bytes();
        let size = bytes.len() as u64;
        let start = cursor.unwrap_or(size.saturating_sub(max_bytes as u64));
        let start = start.min(size) as usize;
        let end = (start + max_bytes).min(bytes.len());
        let slice = bytes.get(start..end).unwrap_or(&[]);
        Some(BackgroundProcessLogData {
            text: String::from_utf8_lossy(slice).into_owned(),
            cursor: end as u64,
            total_written: size,
        })
    }

    fn stop_task(&self, _task_id: &str) -> Result<BackgroundProcessStopData, String> {
        Ok(BackgroundProcessStopData {
            signal_sent: true,
            state: "backgrounded".to_string(),
        })
    }
}

struct StaticSource {
    access: Arc<FakeAccess>,
}

impl crate::domain::background_process_port::BackgroundProcessAccessSource for StaticSource {
    fn current(&self) -> Arc<dyn BackgroundProcessAccess> {
        self.access.clone()
    }
}

fn source() -> Arc<StaticSource> {
    Arc::new(StaticSource {
        access: Arc::new(FakeAccess {
            summaries: vec![BackgroundProcessSummaryData {
                task_id: "task-1".to_string(),
                tool_name: "Bash".to_string(),
                state: "backgrounded".to_string(),
                goal: "跑 cargo test".to_string(),
                duration_ms: Some(1500),
            }],
            log_text: "line-1\nline-2\n".to_string(),
        }),
    })
}

fn test_context() -> crate::domain::context::ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::env::temp_dir()).build()
}

#[tokio::test]
async fn list_tool_returns_summaries() {
    let tool = BackgroundProcessListTool { source: source() };
    let result = tool.call(serde_json::json!({}), &test_context()).await;
    assert!(!result.is_error, "list 不应失败");
    assert_eq!(result.data.expect("结构化数据").tasks.len(), 1);
    assert!(result.text.contains("task-1"));
    assert!(result.text.contains("Bash"));
    assert!(
        result.text.contains("跑 cargo test"),
        "List LLM text MUST 含 goal：{}",
        result.text
    );
}

#[tokio::test]
async fn status_tool_returns_detail() {
    let tool = BackgroundProcessStatusTool { source: source() };
    let result = tool
        .call(serde_json::json!({"task_id": "task-1"}), &test_context())
        .await;
    assert!(!result.is_error);
    let detail = result.data.expect("结构化数据").detail;
    assert_eq!(detail.summary.task_id, "task-1");
    assert_eq!(detail.deadline_remaining_ms, Some(120_000));
}

#[tokio::test]
async fn status_tool_unknown_task_errors() {
    let tool = BackgroundProcessStatusTool { source: source() };
    let result = tool
        .call(serde_json::json!({"task_id": "task-none"}), &test_context())
        .await;
    assert!(result.is_error, "未知任务必须报错");
}

/// 原始缺陷回归：模型按 catalog 注入的发布 schema 传 `timeout` 时，
/// 框架 MUST 在执行前摘除，`BackgroundProcessLogs` 不得因此报未知字段。
#[tokio::test]
async fn logs_tool_accepts_framework_timeout_after_strip() {
    let tool = BackgroundProcessLogsTool { source: source() };
    let mut input = serde_json::json!({"task_id": "task-1", "timeout": 30});
    let taken = crate::domain::timeout_policy::take_timeout_from_input(&mut input);
    assert_eq!(taken, Some(30));

    let result = tool.call(input, &test_context()).await;
    assert!(
        !result.is_error,
        "框架摘除 timeout 后 MUST 正常读取日志: {}",
        result.text
    );
    let log = result.data.expect("结构化数据").log;
    assert!(log.text.contains("line-1"));
}

#[tokio::test]
async fn logs_tool_returns_chunk_and_cursor() {
    let tool = BackgroundProcessLogsTool { source: source() };
    let result = tool
        .call(
            serde_json::json!({"task_id": "task-1", "cursor": 5}),
            &test_context(),
        )
        .await;
    assert!(!result.is_error);
    let log = result.data.expect("结构化数据").log;
    // cursor=5 从 "line-1\nline-2\n" 中部起读，应含后续内容且游标推进到末尾。
    assert!(log.text.contains("line-2"), "增量段：{}", log.text);
    assert!(
        !log.text.starts_with("line-1"),
        "不应重放 cursor 之前的前缀"
    );
    assert_eq!(log.cursor, 14);
}

#[tokio::test]
async fn logs_tool_requires_task_id() {
    let tool = BackgroundProcessLogsTool { source: source() };
    let result = tool.call(serde_json::json!({}), &test_context()).await;
    assert!(result.is_error, "缺 task_id 必须报错");
}

#[tokio::test]
async fn stop_tool_reports_signal() {
    let tool = BackgroundProcessStopTool { source: source() };
    let result = tool
        .call(serde_json::json!({"task_id": "task-1"}), &test_context())
        .await;
    assert!(!result.is_error);
    let stop = result.data.expect("结构化数据").stop;
    assert!(stop.signal_sent);
    assert_eq!(stop.state, "backgrounded");
}

#[test]
fn watch_tool_framework_timeout_is_300_and_immune_to_background_budget() {
    use crate::domain::TypedTool;
    let tool = BackgroundProcessWatchTool { source: source() };
    assert_eq!(tool.get_timeout_secs(None, false), 300);
    // 后台档（24h）不得抬高 Watch：本工具永不转后台。
    assert_eq!(tool.get_timeout_secs(None, true), 300);
    assert_eq!(
        crate::domain::types::background_processes::resolve_watch_wait_secs(Some(60)),
        60
    );
    assert!(
        crate::domain::types::background_processes::resolve_watch_wait_secs(None)
            < tool.get_timeout_secs(None, false),
        "内部 wait MUST 短于框架 timeout"
    );
}

#[tokio::test]
async fn watch_tool_unknown_task_errors() {
    let tool = BackgroundProcessWatchTool { source: source() };
    let result = tool
        .call(serde_json::json!({"task_id": "task-none"}), &test_context())
        .await;
    assert!(result.is_error, "未知任务必须报错");
}

#[tokio::test]
async fn watch_tool_result_text_includes_log_tail_within_budget() {
    let huge = format!("MARKER_HEAD\n{}\nMARKER_TAIL\n", "x".repeat(4_000));
    let source = Arc::new(StaticSource {
        access: Arc::new(FakeAccess {
            summaries: vec![BackgroundProcessSummaryData {
                task_id: "task-done".to_string(),
                tool_name: "Bash".to_string(),
                state: "succeeded".to_string(),
                goal: "Bash · done".to_string(),
                duration_ms: Some(10),
            }],
            log_text: huge,
        }),
    });
    let tool = BackgroundProcessWatchTool { source };
    let result = tool
        .call(serde_json::json!({"task_id": "task-done"}), &test_context())
        .await;
    assert!(!result.is_error);
    let chars = result.text.chars().count();
    assert!(
        chars <= super::super::constants::WATCH_RESULT_TEXT_BUDGET_CHARS,
        "Watch text 必须压在预算内以免 materializer 落盘：{chars}"
    );
    assert!(
        result.text.contains("MARKER_TAIL") || result.text.contains("truncated"),
        "应携带 log tail 或截断标记：{}",
        result.text
    );
    let data = result.data.expect("结构化数据");
    assert!(!data.log_tail.is_empty(), "typed log_tail 不应为空");
    assert!(
        data.log_tail.chars().count() <= super::super::constants::WATCH_RESULT_TEXT_BUDGET_CHARS,
        "log_tail 本身也不得超过总预算"
    );
}

#[tokio::test]
async fn watch_tool_terminal_state_returns_terminal_outcome() {
    let source = Arc::new(StaticSource {
        access: Arc::new(FakeAccess {
            summaries: vec![BackgroundProcessSummaryData {
                task_id: "task-done".to_string(),
                tool_name: "Bash".to_string(),
                state: "succeeded".to_string(),
                goal: "tool=Bash input=ok".to_string(),
                duration_ms: Some(10),
            }],
            log_text: "done-line\n".to_string(),
        }),
    });
    let tool = BackgroundProcessWatchTool { source };
    let result = tool
        .call(serde_json::json!({"task_id": "task-done"}), &test_context())
        .await;
    assert!(!result.is_error);
    let data = result.data.expect("结构化数据");
    assert_eq!(
        data.outcome,
        crate::domain::types::background_processes::BackgroundProcessWatchOutcome::Terminal
    );
    assert!(result.text.contains("done-line"), "终态 text 应含 log tail");
}
