use super::*;
use crate::domain::background_task_port::BackgroundTaskAccess;
use crate::domain::types::background_tasks::{
    BackgroundTaskDetailData, BackgroundTaskLogData, BackgroundTaskStopData,
    BackgroundTaskSummaryData,
};
use std::sync::Arc;

struct FakeAccess {
    summaries: Vec<BackgroundTaskSummaryData>,
}

impl BackgroundTaskAccess for FakeAccess {
    fn list_tasks(&self) -> Vec<BackgroundTaskSummaryData> {
        self.summaries.clone()
    }

    fn task_status(&self, task_id: &str) -> Option<BackgroundTaskDetailData> {
        let summary = self
            .summaries
            .iter()
            .find(|summary| summary.task_id == task_id)?
            .clone();
        Some(BackgroundTaskDetailData {
            summary,
            deadline_remaining_ms: Some(120_000),
            total_written_bytes: 42,
        })
    }

    fn read_task_log(
        &self,
        _task_id: &str,
        _cursor: Option<u64>,
        _max_bytes: usize,
    ) -> Option<BackgroundTaskLogData> {
        Some(BackgroundTaskLogData {
            text: "line-1\nline-2\n".to_string(),
            cursor: 14,
            total_written: 14,
        })
    }

    fn stop_task(&self, _task_id: &str) -> Result<BackgroundTaskStopData, String> {
        Ok(BackgroundTaskStopData {
            signal_sent: true,
            state: "backgrounded".to_string(),
        })
    }
}

struct StaticSource {
    access: Arc<FakeAccess>,
}

impl crate::domain::background_task_port::BackgroundTaskAccessSource for StaticSource {
    fn current(&self) -> Arc<dyn BackgroundTaskAccess> {
        self.access.clone()
    }
}

fn source() -> Arc<StaticSource> {
    Arc::new(StaticSource {
        access: Arc::new(FakeAccess {
            summaries: vec![BackgroundTaskSummaryData {
                task_id: "task-1".to_string(),
                tool_name: "Bash".to_string(),
                state: "backgrounded".to_string(),
                summary: "tool=Bash input=cargo test".to_string(),
                duration_ms: Some(1500),
            }],
        }),
    })
}

fn test_context() -> crate::domain::context::ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::env::temp_dir()).build()
}

#[tokio::test]
async fn list_tool_returns_summaries() {
    let tool = BackgroundTaskListTool { source: source() };
    let result = tool.call(serde_json::json!({}), &test_context()).await;
    assert!(!result.is_error, "list 不应失败");
    assert_eq!(result.data.expect("结构化数据").tasks.len(), 1);
    assert!(result.text.contains("task-1"));
    assert!(result.text.contains("Bash"));
}

#[tokio::test]
async fn status_tool_returns_detail() {
    let tool = BackgroundTaskStatusTool { source: source() };
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
    let tool = BackgroundTaskStatusTool { source: source() };
    let result = tool
        .call(serde_json::json!({"task_id": "task-none"}), &test_context())
        .await;
    assert!(result.is_error, "未知任务必须报错");
}

#[tokio::test]
async fn logs_tool_returns_chunk_and_cursor() {
    let tool = BackgroundTaskLogsTool { source: source() };
    let result = tool
        .call(
            serde_json::json!({"task_id": "task-1", "cursor": 5}),
            &test_context(),
        )
        .await;
    assert!(!result.is_error);
    let log = result.data.expect("结构化数据").log;
    assert!(log.text.contains("line-1"));
    assert_eq!(log.cursor, 14);
}

#[tokio::test]
async fn logs_tool_requires_task_id() {
    let tool = BackgroundTaskLogsTool { source: source() };
    let result = tool.call(serde_json::json!({}), &test_context()).await;
    assert!(result.is_error, "缺 task_id 必须报错");
}

#[tokio::test]
async fn stop_tool_reports_signal() {
    let tool = BackgroundTaskStopTool { source: source() };
    let result = tool
        .call(serde_json::json!({"task_id": "task-1"}), &test_context())
        .await;
    assert!(!result.is_error);
    let stop = result.data.expect("结构化数据").stop;
    assert!(stop.signal_sent);
    assert_eq!(stop.state, "backgrounded");
}
