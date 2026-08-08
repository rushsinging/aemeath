use std::time::Duration;

use audit::{UsageDropReason, UsageEmitOutcome, UsageRecord};
use composition::audit::{usage_worker_config_from_snapshot, AuditUsageSink};
use runtime::UsageSink;
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

#[tokio::test]
async fn audit_usage_sink_forwards_sender_outcomes_without_blocking() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = storage::SafeStorageRoot::open(temp.path()).expect("storage root");
    let store = std::sync::Arc::new(audit::file_usage_append_store(root));
    let (sender, handle) = audit::start_usage_worker(
        store,
        audit::UsageWorkerConfig::new(1, Duration::from_secs(1)),
    );
    let sink = AuditUsageSink::new(sender);
    let record = UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: SessionId::new("01900000-0000-7000-8000-000000000001"),
        run_id: RunId::new("01900000-0000-7000-8000-000000000002"),
        run_step_id: RunStepId::new("01900000-0000-7000-8000-000000000003"),
        model_invocation_id: ModelInvocationId::new("01900000-0000-7000-8000-000000000004"),
        provider: "provider".to_string(),
        model: "model".to_string(),
        input_tokens: 1,
        output_tokens: 2,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    };

    assert_eq!(sink.try_record(record.clone()), UsageEmitOutcome::Accepted);
    handle.shutdown().await;
    assert_eq!(
        sink.try_record(record),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}

#[test]
fn composition_extracts_usage_worker_config_by_value() {
    let mut config = Config::default();
    config.audit.usage_queue_capacity = 17;
    config.audit.usage_shutdown_timeout_ms = 321;
    let snapshot = ConfigSnapshot::new(config);

    let value = usage_worker_config_from_snapshot(&snapshot);
    assert_eq!(value.capacity(), 17);
    assert_eq!(value.shutdown_timeout(), Duration::from_millis(321));
}
