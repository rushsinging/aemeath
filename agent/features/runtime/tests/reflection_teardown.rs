use std::time::Duration;

use runtime::application::reflection::{
    ReflectionTaskAdapter, ReflectionTaskCompletionStatus, ReflectionTaskRequest,
    ReflectionTaskSubmitOutcome, ReflectionTaskTrigger,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn shutdown_before_deadline_preserves_completed_task() {
    let adapter = ReflectionTaskAdapter::new(
        Duration::from_secs(30),
        |_request: ReflectionTaskRequest, _cancel: CancellationToken| async move {
            Ok(runtime::application::reflection::CompleteReflectionResult {
                output: memory::ReflectionOutput::default(),
                formatted_content: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                auto_applied: false,
                apply_result: None,
                error_category: None,
                record_id: None,
            })
        },
    );

    assert_eq!(
        adapter.submit(ReflectionTaskRequest::new(
            ReflectionTaskTrigger::Interval { turn_count: 1 },
            Vec::new(),
        )),
        ReflectionTaskSubmitOutcome::Accepted
    );

    let completions = adapter.shutdown(Duration::from_secs(1)).await;
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].status,
        ReflectionTaskCompletionStatus::Succeeded
    );
}

#[tokio::test]
async fn shutdown_after_deadline_cancels_running_task_and_returns_terminal_completion() {
    let adapter = ReflectionTaskAdapter::new(
        Duration::from_secs(30),
        |_request: ReflectionTaskRequest, cancel: CancellationToken| async move {
            cancel.cancelled().await;
            unreachable!("shutdown cancellation must win the task select")
        },
    );

    assert_eq!(
        adapter.submit(ReflectionTaskRequest::new(
            ReflectionTaskTrigger::PreCompact,
            Vec::new(),
        )),
        ReflectionTaskSubmitOutcome::Accepted
    );

    let completions = adapter.shutdown(Duration::from_millis(50)).await;
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].status,
        ReflectionTaskCompletionStatus::Cancelled,
        "deadline expiry must cancel and wait for the terminal completion"
    );
}
