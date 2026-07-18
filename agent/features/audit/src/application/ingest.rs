use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::{
    AppendLogStream, UsageAppendStorePort, UsageDropReason, UsageEmitOutcome, UsageEnvelopeV1,
    UsageRecord, LOG_TARGET,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageWorkerConfig {
    capacity: usize,
    shutdown_timeout: Duration,
}

impl UsageWorkerConfig {
    pub fn new(capacity: usize, shutdown_timeout: Duration) -> Self {
        Self {
            capacity: capacity.max(1),
            shutdown_timeout: if shutdown_timeout.is_zero() {
                Duration::from_secs(5)
            } else {
                shutdown_timeout
            },
        }
    }

    pub fn capacity(self) -> usize {
        self.capacity
    }

    pub fn shutdown_timeout(self) -> Duration {
        self.shutdown_timeout
    }
}

impl From<share::config::domain::snapshot::UsageWorkerConfig> for UsageWorkerConfig {
    fn from(value: share::config::domain::snapshot::UsageWorkerConfig) -> Self {
        Self::new(value.capacity(), value.shutdown_timeout())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lifecycle {
    Running,
    ShuttingDown,
    Stopped,
}

#[derive(Debug)]
struct PipelineState {
    lifecycle: Lifecycle,
    sender: Option<mpsc::Sender<UsageRecord>>,
    accepted_total: u64,
    completed_total: u64,
    dropped_queue_full_total: u64,
    dropped_worker_unavailable_total: u64,
    write_failed_total: u64,
    drain_abandoned_total: u64,
}

impl PipelineState {
    fn snapshot(&self) -> UsagePipelineMetricsSnapshot {
        UsagePipelineMetricsSnapshot {
            accepted_total: self.accepted_total,
            completed_total: self.completed_total,
            dropped_queue_full_total: self.dropped_queue_full_total,
            dropped_worker_unavailable_total: self.dropped_worker_unavailable_total,
            write_failed_total: self.write_failed_total,
            drain_abandoned_total: self.drain_abandoned_total,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsagePipelineMetricsSnapshot {
    accepted_total: u64,
    completed_total: u64,
    dropped_queue_full_total: u64,
    dropped_worker_unavailable_total: u64,
    write_failed_total: u64,
    drain_abandoned_total: u64,
}

impl UsagePipelineMetricsSnapshot {
    pub fn accepted_total(self) -> u64 {
        self.accepted_total
    }
    pub fn completed_total(self) -> u64 {
        self.completed_total
    }
    pub fn dropped_queue_full_total(self) -> u64 {
        self.dropped_queue_full_total
    }
    pub fn dropped_worker_unavailable_total(self) -> u64 {
        self.dropped_worker_unavailable_total
    }
    pub fn write_failed_total(self) -> u64 {
        self.write_failed_total
    }
    pub fn drain_abandoned_total(self) -> u64 {
        self.drain_abandoned_total
    }
}

#[derive(Clone)]
pub struct UsageSender {
    state: Arc<Mutex<PipelineState>>,
}

impl UsageSender {
    pub fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome {
        let Ok(mut state) = self.state.lock() else {
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        };
        if state.lifecycle != Lifecycle::Running {
            state.dropped_worker_unavailable_total += 1;
            warn_at_threshold("worker_unavailable", state.dropped_worker_unavailable_total);
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        }
        let Some(sender) = state.sender.as_ref().cloned() else {
            state.dropped_worker_unavailable_total += 1;
            warn_at_threshold("worker_unavailable", state.dropped_worker_unavailable_total);
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        };
        match sender.try_send(record) {
            Ok(()) => {
                state.accepted_total += 1;
                UsageEmitOutcome::Accepted
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                state.dropped_queue_full_total += 1;
                warn_at_threshold("queue_full", state.dropped_queue_full_total);
                UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                state.lifecycle = Lifecycle::Stopped;
                state.sender = None;
                state.dropped_worker_unavailable_total += 1;
                warn_at_threshold("worker_unavailable", state.dropped_worker_unavailable_total);
                UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
            }
        }
    }

    pub fn metrics(&self) -> UsagePipelineMetricsSnapshot {
        self.state.lock().unwrap().snapshot()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageShutdownOutcome {
    Drained,
    TimedOut { unconfirmed: u64 },
}

pub struct UsageWorkerHandle {
    state: Arc<Mutex<PipelineState>>,
    timeout: Duration,
    completion: Mutex<Option<UsageShutdownOutcome>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl UsageWorkerHandle {
    pub async fn shutdown(&self) -> UsageShutdownOutcome {
        if let Some(value) = *self.completion.lock().unwrap() {
            return value;
        }
        {
            let mut state = self.state.lock().unwrap();
            if state.lifecycle == Lifecycle::Running {
                state.lifecycle = Lifecycle::ShuttingDown;
                state.sender = None;
            }
        }
        let join = self.join.lock().unwrap().take();
        let outcome = match join {
            Some(mut join) => match tokio::time::timeout(self.timeout, &mut join).await {
                Ok(_) => UsageShutdownOutcome::Drained,
                Err(_) => {
                    join.abort();
                    let mut state = self.state.lock().unwrap();
                    let unconfirmed = state.accepted_total.saturating_sub(state.completed_total);
                    state.drain_abandoned_total += unconfirmed;
                    log::warn!(
                        target: LOG_TARGET,
                        "usage_pipeline kind=drain_timeout cumulative_total={} unconfirmed={unconfirmed}",
                        state.drain_abandoned_total
                    );
                    UsageShutdownOutcome::TimedOut { unconfirmed }
                }
            },
            None => UsageShutdownOutcome::Drained,
        };
        {
            let mut state = self.state.lock().unwrap();
            state.lifecycle = Lifecycle::Stopped;
        }
        *self.completion.lock().unwrap() = Some(outcome);
        outcome
    }
}

pub fn start_usage_worker(
    store: Arc<dyn UsageAppendStorePort>,
    config: UsageWorkerConfig,
) -> (UsageSender, UsageWorkerHandle) {
    let (tx, rx) = mpsc::channel(config.capacity());
    let state = Arc::new(Mutex::new(PipelineState {
        lifecycle: Lifecycle::Running,
        sender: Some(tx),
        accepted_total: 0,
        completed_total: 0,
        dropped_queue_full_total: 0,
        dropped_worker_unavailable_total: 0,
        write_failed_total: 0,
        drain_abandoned_total: 0,
    }));
    let worker_state = Arc::clone(&state);
    let join = tokio::spawn(run_worker(rx, store, worker_state));
    (
        UsageSender {
            state: Arc::clone(&state),
        },
        UsageWorkerHandle {
            state,
            timeout: config.shutdown_timeout(),
            completion: Mutex::new(None),
            join: Mutex::new(Some(join)),
        },
    )
}

async fn run_worker(
    mut receiver: mpsc::Receiver<UsageRecord>,
    store: Arc<dyn UsageAppendStorePort>,
    state: Arc<Mutex<PipelineState>>,
) {
    while let Some(record) = receiver.recv().await {
        let stream = AppendLogStream::for_session(&record.session_id);
        let result = encode(&record).map_err(|_| "encode");
        let failure = match result {
            Ok(bytes) => match store.append(&stream, &bytes).await {
                Ok(()) => match store.flush(&stream).await {
                    Ok(()) => None,
                    Err(_) => Some("flush"),
                },
                Err(_) => Some("append"),
            },
            Err(kind) => Some(kind),
        };
        let mut metrics = state.lock().unwrap();
        if let Some(kind) = failure {
            metrics.write_failed_total += 1;
            warn_at_threshold(kind, metrics.write_failed_total);
        }
        metrics.completed_total += 1;
    }
}

fn encode(record: &UsageRecord) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(&UsageEnvelopeV1::new(record.clone()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn warn_at_threshold(kind: &str, total: u64) {
    if total == 1 || total.is_multiple_of(64) {
        log::warn!(target: LOG_TARGET, "usage_pipeline kind={kind} cumulative_total={total}");
    }
}
