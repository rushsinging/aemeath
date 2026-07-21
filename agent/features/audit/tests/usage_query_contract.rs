use std::num::NonZeroUsize;
use std::sync::Arc;

use audit::{
    file_usage_append_store, usage_query_service, Pagination, TimeRange, UsageCursor,
    UsageEnvelopeV1, UsageQuery, UsageQueryError, UsageQueryPort, UsageQueryWarning, UsageRecord,
};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use storage::SafeStorageRoot;

fn record(session: &str, id: &str, timestamp: u64) -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: timestamp,
        session_id: SessionId::new(session),
        run_id: RunId::new(format!("run-{id}")),
        run_step_id: RunStepId::new(format!("step-{id}")),
        model_invocation_id: ModelInvocationId::new(format!("invocation-{id}")),
        provider: if id == "match" { "anthropic" } else { "openai" }.into(),
        model: if id == "match" {
            "claude-sonnet"
        } else {
            "gpt-4"
        }
        .into(),
        input_tokens: 10,
        output_tokens: 20,
        cache_write_tokens: Some(3),
        cache_read_tokens: None,
        reasoning_tokens: Some(5),
    }
}

fn query(limit: usize) -> UsageQuery {
    UsageQuery {
        session_id: None,
        run_id: None,
        run_step_id: None,
        model_invocation_id: None,
        provider: None,
        model: None,
        recorded_range: None,
        pagination: Pagination {
            cursor: None,
            limit: NonZeroUsize::new(limit).unwrap(),
        },
    }
}

async fn service(
    temp: &tempfile::TempDir,
) -> (Arc<audit::FileUsageAppendStore>, audit::UsageQueryService) {
    let store = Arc::new(file_usage_append_store(
        SafeStorageRoot::open(temp.path()).unwrap(),
    ));
    let query = usage_query_service(store.clone());
    (store, query)
}

async fn append(store: &audit::FileUsageAppendStore, record: UsageRecord) {
    use audit::UsageAppendStorePort;
    let stream = store.stream_for_session(&record.session_id);
    let mut bytes = serde_json::to_vec(&UsageEnvelopeV1::new(record)).unwrap();
    bytes.push(b'\n');
    store.append(&stream, &bytes).await.unwrap();
    store.flush(&stream).await.unwrap();
}

#[tokio::test]
async fn query_filters_by_all_correlation_fields() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service(&temp).await;
    let matching = record("session-a", "match", 15);
    append(&store, matching.clone()).await;
    append(&store, record("session-b", "other", 15)).await;
    let mut request = query(10);
    request.session_id = Some(matching.session_id.clone());
    request.run_id = Some(matching.run_id.clone());
    request.run_step_id = Some(matching.run_step_id.clone());
    request.model_invocation_id = Some(matching.model_invocation_id.clone());
    request.provider = Some(matching.provider.clone());
    request.model = Some(matching.model.clone());
    request.recorded_range = Some(TimeRange {
        from_inclusive_unix_ms: Some(10),
        to_exclusive_unix_ms: Some(20),
    });

    let page = service.query(request).await.unwrap();
    assert_eq!(page.records, vec![matching]);
}

#[tokio::test]
async fn query_paginates_across_sorted_partitions_and_rejects_invalid_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service(&temp).await;
    let first_record = record("session-a", "a", 1);
    let second_record = record("session-b", "b", 2);
    append(&store, first_record.clone()).await;
    append(&store, second_record.clone()).await;
    let mut expected = [first_record, second_record];
    expected.sort_by_key(|record| {
        store
            .stream_for_session(&record.session_id)
            .as_str()
            .to_string()
    });

    let first = service.query(query(1)).await.unwrap();
    assert_eq!(first.records, vec![expected[0].clone()]);
    let mut second_request = query(1);
    second_request.pagination.cursor = first.next_cursor;
    let second = service.query(second_request).await.unwrap();
    assert_eq!(second.records, vec![expected[1].clone()]);
    assert!(second.next_cursor.is_none());

    let mut invalid = query(1);
    invalid.pagination.cursor = Some(UsageCursor::new("not-an-audit-cursor"));
    assert_eq!(
        service.query(invalid).await,
        Err(UsageQueryError::InvalidCursor)
    );

    let first = service.query(query(1)).await.unwrap();
    let mut changed_filter = query(1);
    changed_filter.provider = Some("anthropic".into());
    changed_filter.pagination.cursor = first.next_cursor;
    assert_eq!(
        service.query(changed_filter).await,
        Err(UsageQueryError::InvalidCursor)
    );
}

#[tokio::test]
async fn query_skips_corrupt_and_truncated_lines_then_summarizes_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service(&temp).await;
    let valid = record("session-a", "match", 10);
    append(&store, valid.clone()).await;
    let stream = store.stream_for_session(&valid.session_id);
    let path = temp
        .path()
        .join("usage")
        .join(format!("{}.jsonl", stream.as_str()));
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "not-json").unwrap();
    write!(
        file,
        "{}",
        serde_json::to_string(&UsageEnvelopeV1::new(record("session-a", "tail", 11))).unwrap()
    )
    .unwrap();

    let page = service.query(query(10)).await.unwrap();
    assert_eq!(page.records, vec![valid]);
    assert_eq!(page.warnings.len(), 2);
    assert!(matches!(
        page.warnings[0],
        UsageQueryWarning::CorruptLine { line_number: 2, .. }
    ));
    assert!(matches!(
        page.warnings[1],
        UsageQueryWarning::CorruptLine { line_number: 3, .. }
    ));

    let summary = service.summarize(query(10)).await.unwrap();
    assert_eq!(summary.record_count, 1);
    let mut cursor_query = query(10);
    cursor_query.pagination.cursor = Some(UsageCursor::new("v1:bad:bad:0"));
    assert_eq!(
        service.summarize(cursor_query).await,
        Err(UsageQueryError::InvalidCursor)
    );
    assert_eq!(summary.input_tokens, 10);
    assert_eq!(summary.output_tokens, 20);
    assert_eq!(summary.cache_write_tokens, 3);
    assert_eq!(summary.cache_read_tokens, 0);
    assert_eq!(summary.reasoning_tokens, 5);
}

#[tokio::test]
async fn query_clamps_limit_to_audit_policy_maximum() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service(&temp).await;
    let session = SessionId::new("session-a");
    for index in 0..1_001 {
        append(&store, record(session.as_str(), &index.to_string(), index)).await;
    }

    let page = service.query(query(2_000)).await.unwrap();
    assert_eq!(page.records.len(), 1_000);
    assert!(page.next_cursor.is_some());
}
#[tokio::test]
async fn query_rejects_invalid_range_and_returns_empty_for_missing_partition() {
    let temp = tempfile::tempdir().unwrap();
    let (store, service) = service(&temp).await;
    append(&store, record("session-a", "a", 20)).await;
    let mut request = query(10);
    request.recorded_range = Some(TimeRange {
        from_inclusive_unix_ms: Some(20),
        to_exclusive_unix_ms: Some(20),
    });
    assert_eq!(
        service.query(request).await,
        Err(UsageQueryError::InvalidRange)
    );

    let mut missing = query(10);
    missing.session_id = Some(SessionId::new("missing-session"));
    let page = service.query(missing).await.unwrap();
    assert!(page.records.is_empty());
    assert!(page.warnings.is_empty());
}
