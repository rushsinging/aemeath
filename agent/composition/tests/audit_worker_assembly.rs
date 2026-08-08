use std::time::Duration;

use audit::{UsageDropReason, UsageEmitOutcome, UsageRecord};
use composition::audit::{usage_worker_config_from_snapshot, wire_audit_worker, AuditUsageSink};
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

#[tokio::test]
async fn production_audit_worker_uses_agents_dir_and_remains_live_until_shutdown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let agents_dir = temp.path().join("agents");
    let snapshot = ConfigSnapshot::new(Config::default());
    let assembly = wire_audit_worker(&agents_dir, &snapshot).expect("wire audit worker");
    let sink = AuditUsageSink::new(assembly.sender.clone());
    let record = UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: SessionId::new("01900000-0000-7000-8000-000000000011"),
        run_id: RunId::new("01900000-0000-7000-8000-000000000012"),
        run_step_id: RunStepId::new("01900000-0000-7000-8000-000000000013"),
        model_invocation_id: ModelInvocationId::new("01900000-0000-7000-8000-000000000014"),
        provider: "provider".to_string(),
        model: "model".to_string(),
        input_tokens: 3,
        output_tokens: 5,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    };

    assert_eq!(sink.try_record(record.clone()), UsageEmitOutcome::Accepted);
    assert_eq!(
        assembly.handle.shutdown().await,
        audit::UsageShutdownOutcome::Drained
    );
    assert_eq!(
        sink.try_record(record),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
    assert!(agents_dir
        .join("audit/usage/01900000-0000-7000-8000-000000000011.jsonl")
        .is_file());
}

#[test]
fn production_audit_worker_returns_error_for_unusable_agents_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let agents_dir = temp.path().join("agents-file");
    std::fs::write(&agents_dir, b"not a directory").expect("write blocking file");
    let snapshot = ConfigSnapshot::new(Config::default());

    assert!(wire_audit_worker(&agents_dir, &snapshot).is_err());
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
