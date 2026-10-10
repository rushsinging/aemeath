//! `JsonlSegmentEventStore` 契约测试：日切 jsonl 落盘、读回一致、路径布局与 GC。
use super::*;
use crate::domain::event::{
    ConfigFingerprint, EventActor, EventChange, EventContext, EventOutcome, MemoryEventOp,
};

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-memory-event-jsonl-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn project_key() -> ProjectMemoryKey {
    ProjectMemoryKey::derive("/event/jsonl/store", None).expect("project key")
}

fn test_store(root: &std::path::Path, retention_days: u32) -> JsonlSegmentEventStore {
    JsonlSegmentEventStore::new(
        SafeStorageRoot::open(root).expect("storage root"),
        project_key(),
        retention_days,
    )
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as u64
}

fn day_of(ts_unix_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ts_unix_ms as i64)
        .expect("timestamp in range")
        .format("%Y-%m-%d")
        .to_string()
}

/// 事件落盘的期望目录：`{root}/memory/{project_key}/events/`。
fn expected_events_dir(root: &std::path::Path, project: &ProjectMemoryKey) -> std::path::PathBuf {
    root.join("memory")
        .join(project.as_str())
        .join(EVENTS_SEGMENT)
}

fn sample_event(event_id: &str, ts_unix_ms: u64) -> MemoryEvent {
    MemoryEvent::new(
        event_id,
        ts_unix_ms,
        MemoryEventOp::WriteAdd,
        EventOutcome::Succeeded,
        format!("corr-{event_id}"),
        EventActor::Service,
        EventChange::Write {
            before: vec![],
            after: vec![],
        },
        EventContext {
            query: None,
            trigger_summary: Some(format!("落盘事件 {event_id}")),
            coverage_range: None,
        },
        ConfigFingerprint {
            scoring_enabled: true,
            reflection_model: None,
            reflection_model_revision: None,
            similarity_threshold: None,
            inject_token_budget: None,
            event_retention_days: Some(DEFAULT_EVENT_RETENTION_DAYS),
        },
    )
}

#[tokio::test]
async fn append_one_persists_daily_segment_and_reads_back_equal() {
    let root = unique_root("append-one");
    let store = test_store(&root, DEFAULT_EVENT_RETENTION_DAYS);
    let event = sample_event("evt-append-001", 1_760_000_000_123);

    store.append(&event).await.expect("append one");

    let day_file = expected_events_dir(&root, &project_key())
        .join(format!("{}{EVENTS_JSONL_SUFFIX}", day_of(event.ts_unix_ms)));
    assert!(
        day_file.is_file(),
        "daily segment must exist at {}",
        day_file.display()
    );
    assert_eq!(store.read_all_for_test().expect("read back"), vec![event]);
}

#[tokio::test]
async fn two_appends_preserve_order() {
    let root = unique_root("append-order");
    let store = test_store(&root, DEFAULT_EVENT_RETENTION_DAYS);
    let first = sample_event("evt-order-001", 1_760_000_000_123);
    let second = sample_event("evt-order-002", 1_760_000_004_567);

    store.append(&first).await.expect("append first");
    store.append(&second).await.expect("append second");

    assert_eq!(
        store.read_all_for_test().expect("read back"),
        vec![first, second]
    );
}

#[tokio::test]
async fn segment_path_contains_memory_project_events() {
    let root = unique_root("path-layout");
    let store = test_store(&root, DEFAULT_EVENT_RETENTION_DAYS);
    let event = sample_event("evt-path-001", 1_760_000_000_123);

    store.append(&event).await.expect("append");

    let day_file = expected_events_dir(&root, &project_key())
        .join(format!("{}{EVENTS_JSONL_SUFFIX}", day_of(event.ts_unix_ms)));
    let path = day_file.to_string_lossy().into_owned();
    assert!(
        path.contains(&format!("memory/{}/events/", project_key().as_str())),
        "path {path} must contain memory/{{project}}/events/"
    );
}

#[tokio::test]
async fn gc_deletes_expired_segments_and_keeps_current_day() {
    let root = unique_root("gc-expired");
    let store = test_store(&root, DEFAULT_EVENT_RETENTION_DAYS);
    let now = now_unix_ms();
    let today = sample_event("evt-gc-today", now);
    store.append(&today).await.expect("append today");

    // 植入一个远超保留窗的过期 segment（模拟历史残留）。
    let expired = expected_events_dir(&root, &project_key()).join("2000-01-01.jsonl");
    std::fs::write(&expired, "{}\n").expect("plant expired segment");

    let deleted = store
        .gc_expired(now, DEFAULT_EVENT_RETENTION_DAYS)
        .expect("gc");

    assert_eq!(deleted, 1, "exactly the expired segment is deleted");
    assert!(!expired.exists(), "expired segment must be gone");
    let today_file = expected_events_dir(&root, &project_key())
        .join(format!("{}{EVENTS_JSONL_SUFFIX}", day_of(now)));
    assert!(today_file.is_file(), "today's segment must survive GC");
}

#[tokio::test]
async fn gc_retention_zero_disables_deletion() {
    let root = unique_root("gc-disabled");
    let store = test_store(&root, 0);
    let now = now_unix_ms();

    let expired = expected_events_dir(&root, &project_key()).join("2000-01-01.jsonl");
    std::fs::create_dir_all(expected_events_dir(&root, &project_key())).expect("events dir");
    std::fs::write(&expired, "{}\n").expect("plant expired segment");

    let deleted = store.gc_expired(now, 0).expect("gc disabled");

    assert_eq!(deleted, 0, "retention 0 must never delete");
    assert!(expired.exists(), "segment must survive when GC is disabled");
}

#[tokio::test]
async fn gc_uses_configured_retention_from_constructor() {
    let root = unique_root("gc-configured");
    let store = test_store(&root, DEFAULT_EVENT_RETENTION_DAYS);
    let now = now_unix_ms();

    let expired = expected_events_dir(&root, &project_key()).join("2000-01-01.jsonl");
    std::fs::create_dir_all(expected_events_dir(&root, &project_key())).expect("events dir");
    std::fs::write(&expired, "{}\n").expect("plant expired segment");

    let deleted = store.gc(now).expect("configured gc");

    assert_eq!(
        deleted, 1,
        "configured retention must delete expired segment"
    );
    assert!(!expired.exists(), "expired segment must be gone");
}

#[test]
fn validate_line_rejects_interior_newline_and_reports_no_body() {
    // 缺行尾换行 / 空 payload 均拒绝。
    assert_eq!(
        JsonlSegmentEventStore::validate_line(br#"{"a":1}"#),
        Err(EventAppendError::Rejected)
    );
    assert_eq!(
        JsonlSegmentEventStore::validate_line(b""),
        Err(EventAppendError::Rejected)
    );
    // 行内换行拒绝。
    let error = JsonlSegmentEventStore::validate_line(b"first\nsecond\n")
        .expect_err("interior newline must be rejected");
    // 合法单行通过。
    assert_eq!(
        JsonlSegmentEventStore::validate_line(b"{\"a\":1}\n"),
        Ok(())
    );
    assert!(
        !error.to_string().contains("second"),
        "error Display must never carry payload"
    );
}

#[tokio::test]
async fn wire_memory_event_store_returns_working_append_port() {
    let root = unique_root("wire");
    let port = crate::wire_memory_event_store(
        SafeStorageRoot::open(&root).expect("storage root"),
        project_key(),
        DEFAULT_EVENT_RETENTION_DAYS,
    );
    let event = sample_event("evt-wire-001", 1_760_000_000_123);

    port.append(&event).await.expect("append via wire");

    let day_file = expected_events_dir(&root, &project_key())
        .join(format!("{}{EVENTS_JSONL_SUFFIX}", day_of(event.ts_unix_ms)));
    assert!(day_file.is_file(), "wire-built port must persist events");
}
