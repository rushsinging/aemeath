use std::time::Duration;

use audit::{
    append_store_for, wire_audit_client, wire_audit_store, UsageDropReasonData,
    UsageEmitOutcomeData, UsageQueryData, UsageRecordData,
};
use composition::audit::{wire_session_audit, AuditUsageSink};
use runtime::UsageSink;
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

#[tokio::test]
async fn audit_usage_sink_forwards_sender_outcomes_without_blocking() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = storage::SafeStorageRoot::open(temp.path()).expect("storage root");
    let store = wire_audit_store(append_store_for(root));
    let client = wire_audit_client(&store, 1, Duration::from_secs(1));
    let sink = AuditUsageSink::new(client.clone());
    let record = UsageRecordData {
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

    assert_eq!(
        sink.try_record(record.clone()),
        UsageEmitOutcomeData::Accepted
    );
    client.shutdown().await;
    assert_eq!(
        sink.try_record(record),
        UsageEmitOutcomeData::Dropped(UsageDropReasonData::WorkerUnavailable)
    );
}

#[tokio::test]
async fn production_audit_worker_uses_agents_dir_and_remains_live_until_shutdown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let agents_dir = temp.path().join("agents");
    let snapshot = ConfigSnapshot::new(Config::default());
    let session_audit = wire_session_audit(&agents_dir, &snapshot).expect("wire audit worker");
    let sink = session_audit.usage_sink();
    let record = UsageRecordData {
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

    assert_eq!(
        sink.try_record(record.clone()),
        UsageEmitOutcomeData::Accepted
    );
    session_audit.shutdown().await;
    assert_eq!(
        sink.try_record(record.clone()),
        UsageEmitOutcomeData::Dropped(UsageDropReasonData::WorkerUnavailable)
    );
    let audit_root = storage::SafeStorageRoot::open(agents_dir.join("audit"))
        .expect("reopen production audit root");
    let read_client = wire_audit_client(
        &wire_audit_store(append_store_for(audit_root)),
        1,
        std::time::Duration::from_secs(1),
    );
    let page = read_client
        .query_page(UsageQueryData {
            session_id: Some(record.session_id.clone()),
            run_id: Some(record.run_id.clone()),
            run_step_id: Some(record.run_step_id.clone()),
            model_invocation_id: Some(record.model_invocation_id.clone()),
            provider: Some(record.provider.clone()),
            model: Some(record.model.clone()),
            recorded_range: None,
            pagination: audit::UsagePaginationData {
                cursor: None,
                limit: std::num::NonZeroUsize::new(10).expect("non-zero query limit"),
            },
        })
        .await
        .expect("query drained production record");
    assert_eq!(page.records, vec![record]);
    assert!(page.warnings.is_empty());
}

#[test]
fn production_audit_worker_returns_error_for_unusable_agents_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let agents_dir = temp.path().join("agents-file");
    std::fs::write(&agents_dir, b"not a directory").expect("write blocking file");
    let snapshot = ConfigSnapshot::new(Config::default());

    assert!(wire_session_audit(&agents_dir, &snapshot).is_err());
}

#[test]
fn composition_extracts_usage_worker_config_by_value() {
    let mut config = Config::default();
    config.audit.usage_queue_capacity = 17;
    config.audit.usage_shutdown_timeout_ms = 321;
    let snapshot = ConfigSnapshot::new(config);

    let value = snapshot.usage_worker_config();
    assert_eq!(value.capacity(), 17);
    assert_eq!(value.shutdown_timeout(), Duration::from_millis(321));
}
