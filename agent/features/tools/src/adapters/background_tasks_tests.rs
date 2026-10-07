use super::*;
use crate::domain::background_task_port::BackgroundTaskAccess;
use crate::domain::types::background_tasks::{
    BackgroundTaskDetailData, BackgroundTaskLogData, BackgroundTaskStopData,
    BackgroundTaskSummaryData, BackgroundTasksAction, BackgroundTasksInput,
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

fn tool() -> BackgroundTasksTool {
    BackgroundTasksTool {
        access: Arc::new(FakeAccess {
            summaries: vec![BackgroundTaskSummaryData {
                task_id: "task-1".to_string(),
                tool_name: "Bash".to_string(),
                state: "backgrounded".to_string(),
                summary: "tool=Bash input=cargo test".to_string(),
                duration_ms: Some(1500),
            }],
        }),
    }
}

fn test_context() -> crate::domain::context::ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::env::temp_dir()).build()
}

#[tokio::test]
async fn list_action_returns_summaries() {
    let result = tool()
        .call(serde_json::json!({"action": "list"}), &test_context())
        .await;
    assert!(!result.is_error, "list 不应失败");
    let data = result.data.expect("结构化数据");
    assert_eq!(data.tasks.len(), 1);
    assert_eq!(data.tasks[0].tool_name, "Bash");
    assert!(result.text.contains("task-1"));
}

#[tokio::test]
async fn logs_action_returns_chunk_and_cursor() {
    let result = tool()
        .call(
            serde_json::json!({"action": "logs", "task_id": "task-1", "cursor": 5}),
            &test_context(),
        )
        .await;
    assert!(!result.is_error);
    let data = result.data.expect("结构化数据");
    let log = data.log.expect("日志块");
    assert!(log.text.contains("line-1"));
    assert_eq!(log.cursor, 14);
}

#[tokio::test]
async fn stop_action_reports_signal() {
    let result = tool()
        .call(
            serde_json::json!({"action": "stop", "task_id": "task-1"}),
            &test_context(),
        )
        .await;
    assert!(!result.is_error);
    let data = result.data.expect("结构化数据");
    let stop = data.stop.expect("stop 结果");
    assert!(stop.signal_sent);
    assert_eq!(stop.state, "backgrounded");
}

#[tokio::test]
async fn status_without_task_id_is_input_error() {
    let result = tool()
        .call(serde_json::json!({"action": "status"}), &test_context())
        .await;
    assert!(result.is_error, "status 缺 task_id 必须报错");
}

#[test]
fn input_parses_action_enum() {
    let input: BackgroundTasksInput =
        serde_json::from_str(r#"{"action":"logs","task_id":"task-1","cursor":5}"#).unwrap();
    assert_eq!(input.action, BackgroundTasksAction::Logs);
    assert_eq!(input.cursor, Some(5));
}
