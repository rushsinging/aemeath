use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::domain::{UsageEnvelopeV1, UsageRecordData};
use crate::ports::{AppendLogStream, UsageAppendStorePort};

pub struct UsageWorkerHandle {
    join: Option<JoinHandle<()>>,
}

impl UsageWorkerHandle {
    pub fn new(join: JoinHandle<()>) -> Self {
        Self { join: Some(join) }
    }

    pub async fn shutdown(mut self, timeout: Duration) {
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
}

impl Drop for UsageWorkerHandle {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            join.abort();
            log::warn!(
                target: crate::LOG_TARGET,
                "usage_pipeline kind=owner_dropped_before_shutdown"
            );
        }
    }
}

pub(crate) async fn run_usage_worker(
    mut receiver: mpsc::Receiver<UsageRecordData>,
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

fn encode(record: &UsageRecordData) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(&UsageEnvelopeV1::new(record.clone()))?;
    bytes.push(b'\n');
    Ok(bytes)
}
