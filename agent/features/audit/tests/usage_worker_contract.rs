use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use audit::{
    start_usage_worker, AppendLogError, AppendLogNamespace, AppendLogReader, AppendLogStream,
    UsageAppendStorePort, UsageDropReason, UsageEmitOutcome, UsageRecord, UsageShutdownOutcome,
    UsageWorkerConfig,
};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};

#[derive(Default)]
struct SpyStore {
    calls: Mutex<Vec<String>>,
    fail_append: bool,
    delay: Option<Duration>,
}

#[async_trait]
impl UsageAppendStorePort for SpyStore {
    async fn append(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError> {
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        self.calls.lock().unwrap().push(format!(
            "append:{}:{}",
            stream.as_str(),
            bytes.ends_with(b"\n")
        ));
        if self.fail_append {
            Err(AppendLogError::Io)
        } else {
            Ok(())
        }
    }

    async fn flush(&self, stream: &AppendLogStream) -> Result<(), AppendLogError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("flush:{}", stream.as_str()));
        Ok(())
    }

    async fn read(&self, _: &AppendLogStream) -> Result<AppendLogReader, AppendLogError> {
        unreachable!()
    }

    async fn list_streams(
        &self,
        _: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError> {
        unreachable!()
    }
}

fn record(id: &str) -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: SessionId::new(format!("session-{id}")),
        run_id: RunId::new(format!("run-{id}")),
        run_step_id: RunStepId::new(format!("step-{id}")),
        model_invocation_id: ModelInvocationId::new(format!("inv-{id}")),
        provider: "test".into(),
        model: "test".into(),
        input_tokens: 1,
        output_tokens: 1,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    }
}

#[tokio::test]
async fn worker_appends_then_flushes_and_drains_on_shutdown() {
    let store = Arc::new(SpyStore::default());
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(4, Duration::from_secs(1)),
    );
    assert_eq!(sender.try_record(record("a")), UsageEmitOutcome::Accepted);
    assert_eq!(sender.try_record(record("b")), UsageEmitOutcome::Accepted);

    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    let calls = store.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 4);
    assert!(calls[0].starts_with("append:"));
    assert!(calls[1].starts_with("flush:"));
    assert!(calls[2].starts_with("append:"));
    assert!(calls[3].starts_with("flush:"));
    assert_eq!(
        sender.try_record(record("late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );

    let metrics = sender.metrics();
    assert_eq!(metrics.accepted_total(), 2);
    assert_eq!(metrics.completed_total(), 2);
    assert_eq!(metrics.write_failed_total(), 0);
}

#[tokio::test]
async fn bounded_sender_reports_queue_full_without_waiting() {
    let store = Arc::new(SpyStore {
        delay: Some(Duration::from_millis(100)),
        ..Default::default()
    });
    let (sender, handle) =
        start_usage_worker(store, UsageWorkerConfig::new(1, Duration::from_secs(1)));
    let _ = sender.try_record(record("one"));
    let mut saw_full = false;
    for index in 0..32 {
        if sender.try_record(record(&index.to_string()))
            == UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
        {
            saw_full = true;
            break;
        }
    }
    assert!(saw_full);
    assert!(sender.metrics().dropped_queue_full_total() >= 1);
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn write_failure_is_counted_once_and_does_not_stop_drain() {
    let store = Arc::new(SpyStore {
        fail_append: true,
        ..Default::default()
    });
    let (sender, handle) =
        start_usage_worker(store, UsageWorkerConfig::new(4, Duration::from_secs(1)));
    assert_eq!(sender.try_record(record("a")), UsageEmitOutcome::Accepted);
    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    let metrics = sender.metrics();
    assert_eq!(metrics.write_failed_total(), 1);
    assert_eq!(metrics.completed_total(), 1);
}

#[tokio::test]
async fn shutdown_timeout_reports_unconfirmed_and_is_idempotent() {
    let store = Arc::new(SpyStore {
        delay: Some(Duration::from_secs(1)),
        ..Default::default()
    });
    let (sender, handle) =
        start_usage_worker(store, UsageWorkerConfig::new(4, Duration::from_millis(10)));
    assert_eq!(sender.try_record(record("a")), UsageEmitOutcome::Accepted);
    let first = handle.shutdown().await;
    assert!(matches!(first, UsageShutdownOutcome::TimedOut { unconfirmed } if unconfirmed >= 1));
    assert_eq!(handle.shutdown().await, first);
    assert!(sender.metrics().drain_abandoned_total() >= 1);
}
