use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use audit::{
    start_usage_worker, AppendLogError, AppendLogNamespace, AppendLogReader, AppendLogStream,
    UsageAppendStorePort, UsageDropReason, UsageEmitOutcome, UsageRecord, UsageWorkerConfig,
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
async fn public_worker_partitions_records_drains_once_and_rejects_late_records() {
    let store = Arc::new(RecordingStore::default());
    let (sender, worker) = start_usage_worker(
        store.clone(),
        UsageWorkerConfig::new(4, Duration::from_secs(1)),
    );
    let first = record("a");
    let second = record("b");
    let first_stream = AppendLogStream::for_session(&first.session_id)
        .as_str()
        .to_string();
    let second_stream = AppendLogStream::for_session(&second.session_id)
        .as_str()
        .to_string();

    assert_eq!(sender.try_record(first), UsageEmitOutcome::Accepted);
    assert_eq!(sender.try_record(second), UsageEmitOutcome::Accepted);
    worker.shutdown().await;

    assert_eq!(
        store.calls.lock().expect("store calls lock").as_slice(),
        [
            format!("append:{first_stream}:true"),
            format!("flush:{first_stream}"),
            format!("append:{second_stream}:true"),
            format!("flush:{second_stream}"),
        ]
    );
    assert_eq!(
        sender.try_record(record("late")),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}
