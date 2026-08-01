use super::*;
use std::{future::pending, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

fn successful_payload() -> CompleteReflectionResult {
    CompleteReflectionResult {
        output: memory::api::ReflectionOutput::default(),
        input_tokens: 0,
        output_tokens: 0,
        apply_result: None,
        error_category: None,
        record_id: None,
    }
}

fn request() -> ReflectionTaskRequest {
    ReflectionTaskRequest::new(ReflectionTaskTrigger::PreCompact, vec![])
}

fn assert_completion(
    completions: Vec<ReflectionTaskCompletion>,
    expected: ReflectionTaskCompletionStatus,
) {
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].status, expected);
}

#[tokio::test]
async fn first_task_claims_slot_and_second_submission_skips_without_waiting() {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
        let started = Arc::clone(&started);
        let release = Arc::clone(&release);
        move |_request, _cancel| {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            async move {
                started.notify_one();
                release.notified().await;
                Ok(successful_payload())
            }
        }
    });

    assert_eq!(
        adapter.submit(request()),
        ReflectionTaskSubmitOutcome::Accepted
    );
    started.notified().await;
    assert_eq!(
        adapter.submit(request()),
        ReflectionTaskSubmitOutcome::BusySkipped
    );
    release.notify_one();
    assert_completion(
        adapter.drain().await,
        ReflectionTaskCompletionStatus::Succeeded,
    );
}

#[tokio::test]
async fn submission_captures_current_dependencies_each_time() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));

    for marker in ["initial", "switched"] {
        let observed = Arc::clone(&observed);
        let marker = marker.to_string();
        assert_eq!(
            adapter.submit_future(
                ReflectionTaskTrigger::PreCompact,
                move |_cancel| async move {
                    observed.lock().await.push(marker);
                    Ok(successful_payload())
                }
            ),
            ReflectionTaskSubmitOutcome::Accepted
        );
        assert_completion(
            adapter.drain().await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
    }

    assert_eq!(*observed.lock().await, ["initial", "switched"]);
}

#[tokio::test]
async fn owned_request_snapshot_is_stable_after_submission() {
    let observed = Arc::new(Mutex::new(Vec::<String>::new()));
    let release = Arc::new(Notify::new());
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
        let observed = Arc::clone(&observed);
        let release = Arc::clone(&release);
        move |request: ReflectionTaskRequest, _cancel| {
            let observed = Arc::clone(&observed);
            let release = Arc::clone(&release);
            async move {
                release.notified().await;
                *observed.lock().await = request
                    .messages
                    .iter()
                    .map(share::message::Message::text_content)
                    .collect();
                Ok(successful_payload())
            }
        }
    });
    let mut live = vec![share::message::Message::user("before compact")];
    let frozen = ReflectionTaskRequest::new(ReflectionTaskTrigger::PreCompact, live.clone());
    assert_eq!(
        adapter.submit(frozen),
        ReflectionTaskSubmitOutcome::Accepted
    );
    live.push(share::message::Message::user("after submit"));
    release.notify_one();
    let _ = adapter.drain().await;
    assert_eq!(*observed.lock().await, ["before compact"]);
}

#[tokio::test]
async fn cancel_releases_slot_and_reports_cancelled() {
    let started = Arc::new(Notify::new());
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
        let started = Arc::clone(&started);
        move |_request, cancel: CancellationToken| {
            let started = Arc::clone(&started);
            async move {
                started.notify_one();
                cancel.cancelled().await;
                Ok(successful_payload())
            }
        }
    });
    assert_eq!(
        adapter.submit(request()),
        ReflectionTaskSubmitOutcome::Accepted
    );
    started.notified().await;
    adapter.cancel().await;
    assert_completion(
        adapter.drain().await,
        ReflectionTaskCompletionStatus::Cancelled,
    );
    assert_eq!(
        adapter.submit(request()),
        ReflectionTaskSubmitOutcome::Accepted
    );
    adapter.cancel().await;
    let _ = adapter.drain().await;
}

#[tokio::test]
async fn timeout_releases_slot_and_reports_timed_out() {
    let adapter =
        ReflectionTaskAdapter::new(Duration::from_millis(20), |_request, _cancel| async move {
            pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(successful_payload())
        });
    assert_eq!(
        adapter.submit(request()),
        ReflectionTaskSubmitOutcome::Accepted
    );
    assert_completion(
        adapter.drain().await,
        ReflectionTaskCompletionStatus::TimedOut,
    );
    assert_eq!(
        adapter.submit(request()),
        ReflectionTaskSubmitOutcome::Accepted
    );
    adapter.cancel().await;
    let _ = adapter.drain().await;
}
