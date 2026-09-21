use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use tokio::sync::{mpsc, Semaphore};

use super::ingest::{start_usage_worker, UsageWorkerConfig};
use crate::{
    AppendLogError, AppendLogNamespace, AppendLogReader, AppendLogStream, UsageAppendStorePort,
    UsageDropReason, UsageEmitOutcome, UsageRecord,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoreCall {
    Append { stream: String, terminated: bool },
    Flush { stream: String },
}

struct AppendDropNotice(Option<mpsc::UnboundedSender<()>>);

impl Drop for AppendDropNotice {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

struct ControlledStore {
    calls: Mutex<Vec<StoreCall>>,
    append_started: mpsc::UnboundedSender<()>,
    append_dropped: mpsc::UnboundedSender<()>,
    allow_append: Semaphore,
    fail_append: bool,
    fail_flush: bool,
}

impl ControlledStore {
    fn new(
        fail_append: bool,
        fail_flush: bool,
    ) -> (
        Arc<Self>,
        mpsc::UnboundedReceiver<()>,
        mpsc::UnboundedReceiver<()>,
    ) {
        let (append_started, started_receiver) = mpsc::unbounded_channel();
        let (append_dropped, dropped_receiver) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                append_started,
                append_dropped,
                allow_append: Semaphore::new(0),
                fail_append,
                fail_flush,
            }),
            started_receiver,
            dropped_receiver,
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
        let _drop_notice = AppendDropNotice(Some(self.append_dropped.clone()));
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

fn record(session_id: &str, id: &str) -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: SessionId::new(session_id),
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
    let (store, mut append_started, _append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(1, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    assert_eq!(
        sender.try_record(record("session-a", "queued")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("session-a", "overflow")),
        UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
    );

    store.release_append();
    append_started.recv().await.expect("queued append starts");
    store.release_append();
    worker.shutdown().await;
}

#[tokio::test]
async fn worker_partitions_each_record_by_session_and_drains_in_fifo_order() {
    let (store, mut append_started, _append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );
    let first = record("session-a", "first");
    let second = record("session-b", "second");

    assert_eq!(sender.try_record(first.clone()), UsageEmitOutcome::Accepted);
    assert_eq!(
        sender.try_record(second.clone()),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    worker.shutdown().await;
    let first_stream = AppendLogStream::for_session(&first.session_id)
        .as_str()
        .to_string();
    let second_stream = AppendLogStream::for_session(&second.session_id)
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
}

#[tokio::test]
async fn append_failure_skips_flush_and_continues_with_next_record() {
    let (store, mut append_started, _append_dropped) = ControlledStore::new(true, false);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("session-a", "second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    worker.shutdown().await;
    let calls = store.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|call| matches!(call, StoreCall::Append { .. })));
}

#[tokio::test]
async fn flush_failure_continues_with_next_record() {
    let (store, mut append_started, _append_dropped) = ControlledStore::new(false, true);
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(2, Duration::from_secs(1)),
    );

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    assert_eq!(
        sender.try_record(record("session-a", "second")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("first append starts");
    store.release_append();
    append_started.recv().await.expect("second append starts");
    store.release_append();

    worker.shutdown().await;
    let calls = store.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, StoreCall::Append { .. }))
            .count(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, StoreCall::Flush { .. }))
            .count(),
        2
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_timeout_aborts_blocked_worker_and_closes_sender() {
    let (store, mut append_started, mut append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) =
        start_usage_worker(store, UsageWorkerConfig::new(2, Duration::from_secs(1)));

    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("append starts");

    worker.shutdown().await;

    append_dropped
        .recv()
        .await
        .expect("blocked append future drops");
    assert_eq!(
        sender.try_record(record("session-a", "late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}

#[tokio::test]
async fn cancelling_shutdown_aborts_worker_instead_of_detaching_it() {
    let (store, mut append_started, mut append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) =
        start_usage_worker(store, UsageWorkerConfig::new(1, Duration::from_secs(30)));
    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("append starts");

    let shutdown_task = tokio::spawn(worker.shutdown());
    tokio::task::yield_now().await;
    shutdown_task.abort();
    let _ = shutdown_task.await;

    append_dropped
        .recv()
        .await
        .expect("append future drops on cancellation");
    assert_eq!(
        sender.try_record(record("session-a", "late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}

#[tokio::test]
async fn dropping_worker_owner_aborts_worker_and_closes_sender() {
    let (store, mut append_started, mut append_dropped) = ControlledStore::new(false, false);
    let (sender, worker) =
        start_usage_worker(store, UsageWorkerConfig::new(1, Duration::from_secs(30)));
    assert_eq!(
        sender.try_record(record("session-a", "first")),
        UsageEmitOutcome::Accepted
    );
    append_started.recv().await.expect("append starts");

    drop(worker);

    append_dropped
        .recv()
        .await
        .expect("append future drops with owner");
    assert_eq!(
        sender.try_record(record("session-a", "late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}
