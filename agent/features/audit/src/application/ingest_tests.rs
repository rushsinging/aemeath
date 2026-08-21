use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use tokio::sync::{mpsc, Semaphore};

use super::ingest::{start_usage_worker, UsageWorkerConfig};
use crate::{
    AppendLogError, AppendLogNamespace, AppendLogReader, AppendLogStream, UsageAppendStorePort,
    UsageDropReason, UsageEmitOutcome, UsageRecord, UsageShutdownOutcome,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoreCall {
    Append { stream: String, terminated: bool },
    Flush { stream: String },
}

struct ControlledStore {
    calls: Mutex<Vec<StoreCall>>,
    append_started: mpsc::UnboundedSender<()>,
    allow_append: Semaphore,
    fail_append: bool,
    fail_flush: bool,
}

impl ControlledStore {
    fn new(fail_append: bool, fail_flush: bool) -> (Arc<Self>, mpsc::UnboundedReceiver<()>) {
        let (append_started, receiver) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                append_started,
                allow_append: Semaphore::new(0),
                fail_append,
                fail_flush,
            }),
            receiver,
        )
    }

    fn release_append(&self) {
        self.allow_append.add_permits(1);
    }

    fn calls(&self) -> Vec<StoreCall> {
        self.calls.lock().expect("store calls lock").clone()
    }
}

#[async_trait]
impl UsageAppendStorePort for ControlledStore {
    async fn append(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError> {
        self.calls
            .lock()
            .expect("store calls lock")
            .push(StoreCall::Append {
                stream: stream.as_str().to_string(),
                terminated: bytes.ends_with(b"\n"),
            });
        self.append_started
            .send(())
            .expect("append observer remains available");
        self.allow_append
            .acquire()
            .await
            .expect("append permit semaphore remains open")
            .forget();
        if self.fail_append {
            Err(AppendLogError::Io)
        } else {
            Ok(())
        }
    }

    async fn flush(&self, stream: &AppendLogStream) -> Result<(), AppendLogError> {
        self.calls
            .lock()
            .expect("store calls lock")
            .push(StoreCall::Flush {
                stream: stream.as_str().to_string(),
            });
        if self.fail_flush {
            Err(AppendLogError::Io)
        } else {
            Ok(())
        }
    }

    async fn read(&self, _: &AppendLogStream) -> Result<AppendLogReader, AppendLogError> {
        Err(AppendLogError::Closed)
    }

    async fn list_streams(
        &self,
        _: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError> {
        Err(AppendLogError::Closed)
    }
}

fn record(id: &str) -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: SessionId::new(format!("session-{id}")),
        run_id: RunId::new(format!("run-{id}")),
        run_step_id: RunStepId::new(format!("step-{id}")),
        model_invocation_id: ModelInvocationId::new(format!("invocation-{id}")),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        input_tokens: 1,
        output_tokens: 2,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    }
}

#[test]
fn worker_config_enforces_capacity_floor_and_zero_timeout_default() {
    let config = UsageWorkerConfig::new(0, Duration::ZERO);

    assert_eq!(config.capacity(), 1);
    assert_eq!(config.shutdown_timeout(), Duration::from_secs(5));
}

#[tokio::test]
async fn full_queue_drops_immediately_while_first_append_is_blocked() {
    let (store, mut append_started) = ControlledStore::new(false, false);
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(1, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    assert_eq!(
        sender.try_record(record("queued")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("overflow")),
        UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
    );

    store.release_append();
    append_started.recv().await.expect("queued append starts");
    store.release_append();
    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    assert_eq!(sender.metrics().dropped_queue_full_total(), 1);
}

#[tokio::test]
async fn worker_calls_append_then_flush_in_fifo_order_and_drains() {
    let (store, mut append_started) = ControlledStore::new(false, false);
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    let first_stream = AppendLogStream::for_session(&record("first").session_id)
        .as_str()
        .to_string();
    let second_stream = AppendLogStream::for_session(&record("second").session_id)
        .as_str()
        .to_string();
    assert_eq!(
        store.calls(),
        vec![
            StoreCall::Append {
                stream: first_stream.clone(),
                terminated: true,
            },
            StoreCall::Flush {
                stream: first_stream,
            },
            StoreCall::Append {
                stream: second_stream.clone(),
                terminated: true,
            },
            StoreCall::Flush {
                stream: second_stream,
            },
        ]
    );
    let metrics = sender.metrics();
    assert_eq!(metrics.accepted_total(), 2);
    assert_eq!(metrics.completed_total(), 2);
    assert_eq!(metrics.write_failed_total(), 0);
}

#[tokio::test]
async fn append_failure_skips_flush_counts_each_record_and_continues() {
    let (store, mut append_started) = ControlledStore::new(true, false);
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    assert!(store
        .calls()
        .iter()
        .all(|call| matches!(call, StoreCall::Append { .. })));
    let metrics = sender.metrics();
    assert_eq!(metrics.accepted_total(), 2);
    assert_eq!(metrics.completed_total(), 2);
    assert_eq!(metrics.write_failed_total(), 2);
}

#[tokio::test]
async fn flush_failure_counts_each_record_and_continues() {
    let (store, mut append_started) = ControlledStore::new(false, true);
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    assert_eq!(
        store
            .calls()
            .iter()
            .filter(|call| matches!(call, StoreCall::Flush { .. }))
            .count(),
        2
    );
    let metrics = sender.metrics();
    assert_eq!(metrics.completed_total(), 2);
    assert_eq!(metrics.write_failed_total(), 2);
}

#[tokio::test(start_paused = true)]
async fn shutdown_timeout_counts_exact_unconfirmed_records_and_is_idempotent() {
    let (store, mut append_started) = ControlledStore::new(false, false);
    let (sender, handle) =
        start_usage_worker(store, UsageWorkerConfig::new(2, Duration::from_secs(1)));

    assert_eq!(
        sender.try_record(record("first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");

    let first = handle.shutdown().await;
    assert_eq!(first, UsageShutdownOutcome::TimedOut { unconfirmed: 2 });
    assert_eq!(handle.shutdown().await, first);
    let metrics = sender.metrics();
    assert_eq!(metrics.accepted_total(), 2);
    assert_eq!(metrics.completed_total(), 0);
    assert_eq!(metrics.drain_abandoned_total(), 2);
}

#[tokio::test]
async fn sender_rejects_after_shutdown_and_metrics_preserve_terminal_conservation() {
    let (store, mut append_started) = ControlledStore::new(false, false);
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(1, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    assert_eq!(
        sender.try_record(record("late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );

    let metrics = sender.metrics();
    assert_eq!(metrics.accepted_total(), metrics.completed_total());
    assert_eq!(metrics.drain_abandoned_total(), 0);
    assert_eq!(metrics.dropped_worker_unavailable_total(), 1);
}
