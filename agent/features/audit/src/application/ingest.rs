use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::{
    AppendLogStream, UsageAppendStorePort, UsageDropReason, UsageEmitOutcome, UsageEnvelopeV1,
    UsageRecord,
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

type UsageSenderSlot = Arc<Mutex<Option<mpsc::Sender<UsageRecord>>>>;

#[derive(Clone)]
pub struct UsageSender {
    sender: UsageSenderSlot,
}

impl UsageSender {
    pub fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome {
        let Ok(mut sender_slot) = self.sender.lock() else {
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        };
        let Some(sender) = sender_slot.as_ref() else {
            return UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable);
        };
        match sender.try_send(record) {
            Ok(()) => UsageEmitOutcome::Accepted,
            Err(mpsc::error::TrySendError::Full(_)) => {
                UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                *sender_slot = None;
                UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
            }
        }
    }
}

pub struct UsageWorker {
    sender: UsageSenderSlot,
    timeout: Duration,
    join: Option<JoinHandle<()>>,
}

impl UsageWorker {
    pub async fn shutdown(mut self) {
        self.close_sender();
        let timeout = self.timeout;
        let Some(join) = self.join.as_mut() else {
            return;
        };
        if tokio::time::timeout(timeout, join).await.is_err() {
            self.join
                .as_ref()
                .expect("usage worker join remains owned during timeout")
                .abort();
            let _ = self
                .join
                .as_mut()
                .expect("aborted usage worker join remains owned")
                .await;
            log::warn!(
                target: crate::LOG_TARGET,
                "usage_pipeline kind=shutdown_timeout"
            );
        }
        self.join = None;
    }

    fn close_sender(&self) {
        if let Ok(mut sender_slot) = self.sender.lock() {
            sender_slot.take();
        }
    }
}

impl Drop for UsageWorker {
    fn drop(&mut self) {
        self.close_sender();
        if let Some(join) = self.join.take() {
            join.abort();
            log::warn!(
                target: crate::LOG_TARGET,
                "usage_pipeline kind=owner_dropped_before_shutdown"
            );
        }
    }
}

pub fn start_usage_worker(
    store: Arc<dyn UsageAppendStorePort>,
    config: UsageWorkerConfig,
) -> (UsageSender, UsageWorker) {
    let (sender, receiver) = mpsc::channel(config.capacity());
    let sender = Arc::new(Mutex::new(Some(sender)));
    let join = tokio::spawn(run_worker(receiver, store));
    (
        UsageSender {
            sender: Arc::clone(&sender),
        },
        UsageWorker {
            sender,
            timeout: config.shutdown_timeout(),
            join: Some(join),
        },
    )
}

async fn run_worker(
    mut receiver: mpsc::Receiver<UsageRecord>,
    store: Arc<dyn UsageAppendStorePort>,
) {
    while let Some(record) = receiver.recv().await {
        let stream = AppendLogStream::for_session(&record.session_id);
        let bytes = match encode(&record) {
            Ok(bytes) => bytes,
            Err(_) => {
                log::warn!(target: crate::LOG_TARGET, "usage_pipeline kind=encode");
                continue;
            }
        };
        if store.append(&stream, &bytes).await.is_err() {
            log::warn!(target: crate::LOG_TARGET, "usage_pipeline kind=append");
            continue;
        }
        if store.flush(&stream).await.is_err() {
            log::warn!(target: crate::LOG_TARGET, "usage_pipeline kind=flush");
        }
    }
}

fn encode(record: &UsageRecord) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(&UsageEnvelopeV1::new(record.clone()))?;
    bytes.push(b'\n');
    Ok(bytes)
}
