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
struct RecordingStore {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl UsageAppendStorePort for RecordingStore {
    async fn append(&self, stream: &AppendLogStream, bytes: &[u8]) -> Result<(), AppendLogError> {
        self.calls.lock().expect("store calls lock").push(format!(
            "append:{}:{}",
            stream.as_str(),
            bytes.ends_with(b"\n")
        ));
        Ok(())
    }

    async fn flush(&self, stream: &AppendLogStream) -> Result<(), AppendLogError> {
        self.calls
            .lock()
            .expect("store calls lock")
            .push(format!("flush:{}", stream.as_str()));
        Ok(())
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
        model_invocation_id: ModelInvocationId::new(format!("inv-{id}")),
        provider: "test".to_string(),
        model: "test".to_string(),
        input_tokens: 1,
        output_tokens: 1,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    }
}

#[tokio::test]
async fn public_worker_appends_flushes_drains_and_rejects_late_records() {
    let store = Arc::new(RecordingStore::default());
    let (sender, handle) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(4, Duration::from_secs(1)),
    );
    assert_eq!(sender.try_record(record("a")), UsageEmitOutcome::Accepted);
    assert_eq!(sender.try_record(record("b")), UsageEmitOutcome::Accepted);

    assert_eq!(handle.shutdown().await, UsageShutdownOutcome::Drained);
    let calls = store.calls.lock().expect("store calls lock").clone();
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
    assert_eq!(metrics.dropped_worker_unavailable_total(), 1);
}
