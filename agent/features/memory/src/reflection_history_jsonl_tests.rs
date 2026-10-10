//! `JsonlReflectionHistoryStore` 契约测试：append-only 落盘、读侧 id 折叠、
//! legacy AtomicDataset 一次性导出与日切 segment GC。
use std::io::Write as _;

use super::*;
use crate::domain::{ReflectionErrorCategory, ReflectionTrigger};

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-reflection-history-jsonl-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn project_key() -> ProjectMemoryKey {
    ProjectMemoryKey::derive("/reflection/history/jsonl", None).expect("project key")
}

fn test_store(root: &std::path::Path, retention_days: u32) -> JsonlReflectionHistoryStore {
    JsonlReflectionHistoryStore::new(
        SafeStorageRoot::open(root).expect("storage root"),
        project_key(),
        retention_days,
        None,
    )
}

fn record(id: &str, timestamp: u64) -> ReflectionRecord {
    ReflectionRecord::failed(
        id,
        timestamp,
        ReflectionTrigger::Manual,
        ReflectionErrorCategory::TimedOut,
        25,
    )
}

/// `ReflectionRecord::timestamp`（UTC 秒）所在 UTC 日。
fn day_of(timestamp_secs: u64) -> String {
    chrono::DateTime::from_timestamp_millis(
        i64::try_from(timestamp_secs).expect("ts in range") * 1000,
    )
    .expect("timestamp in range")
    .format("%Y-%m-%d")
    .to_string()
}

/// 落盘的期望目录：`{root}/memory/{project_key}/reflection-history/`。
fn expected_history_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join("memory")
        .join(project_key().as_str())
        .join(REFLECTION_HISTORY_SEGMENT)
}

fn segment_path(root: &std::path::Path, timestamp_secs: u64) -> std::path::PathBuf {
    expected_history_dir(root).join(format!(
        "{}{REFLECTION_HISTORY_JSONL_SUFFIX}",
        day_of(timestamp_secs)
    ))
}

/// 预置 legacy AtomicDataset：dataset key `memory/{project}/reflection-history`
/// 下写入 member `records` = `Vec<ReflectionRecord>` JSON。
async fn seed_legacy_records(
    root: &std::path::Path,
    records: &[ReflectionRecord],
) -> Arc<dyn storage::AtomicDatasetPort> {
    let port = storage::wire_file_system_dataset(root).expect("dataset port");
    seed_legacy_bytes(
        &port,
        &serde_json::to_vec(records).expect("serialize records"),
    )
    .await;
    port
}

async fn seed_legacy_bytes(port: &Arc<dyn storage::AtomicDatasetPort>, bytes: &[u8]) {
    let key = storage::DatasetKeyData::new(
        storage::StorageNamespaceData::Memory,
        vec![
            SafePathSegmentData::from_str(project_key().as_str()).expect("project segment"),
            SafePathSegmentData::from_str(REFLECTION_HISTORY_SEGMENT).expect("history segment"),
        ],
    )
    .expect("history dataset key");
    let manifest = port.read_manifest(&key).await.expect("read manifest");
    port.commit_atomic(
        &key,
        manifest.revision(),
        &[storage::DatasetMemberData::new(
            SafePathSegmentData::from_str(REFLECTION_RECORDS_MEMBER).expect("records member"),
            bytes.to_vec(),
        )],
        storage::WriteOptionsData::new(storage::DurabilityData::ProcessCrashSafe),
    )
    .await
    .expect("seed legacy records member");
}

#[test]
fn jsonl_store_is_memory_owned_double_trait() {
    fn assert_store<T: ReflectionHistoryStore + ReflectionHistoryQuery>() {}
    assert_store::<JsonlReflectionHistoryStore>();
}

/// TDD #1：append 两个不同 id → list(10) 返回两条，newest append first。
#[tokio::test]
async fn append_two_ids_lists_both_newest_first() {
    let root = unique_root("append-two");
    let store = test_store(&root, 30);

    store.append(&record("first", 10)).await.unwrap();
    store.append(&record("second", 20)).await.unwrap();

    assert_eq!(
        store.list(10).await.unwrap(),
        vec![
            record("second", 20).safe_summary(),
            record("first", 10).safe_summary(),
        ]
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// TDD #2：upsert 同 id 两次 → 文件两行（取消原地替换），list 折叠为最新内容。
#[tokio::test]
async fn upsert_same_id_appends_second_line_and_read_folds_to_latest() {
    let root = unique_root("upsert-lines");
    let store = test_store(&root, 30);
    let running = ReflectionRecord::running("stable", 40, ReflectionTrigger::Manual);
    store.append(&running).await.unwrap();
    let terminal = record("stable", 40);
    store.upsert(&terminal).await.unwrap();

    let segment = segment_path(&root, 40);
    let content = std::fs::read_to_string(&segment).expect("segment readable");
    assert_eq!(
        content.lines().count(),
        2,
        "upsert 必须再追加一行而非原地替换"
    );

    assert_eq!(store.list(10).await.unwrap(), vec![terminal.safe_summary()]);
    std::fs::remove_dir_all(root).unwrap();
}

/// TDD #3：预置旧 AtomicDataset `records` → 首次读即迁移导出到 jsonl 且可读。
#[tokio::test]
async fn legacy_records_member_migrates_to_jsonl_on_first_read() {
    let root = unique_root("legacy-migrate");
    let legacy_record = record("legacy-1", 10);
    let port = seed_legacy_records(&root, std::slice::from_ref(&legacy_record)).await;

    let store = JsonlReflectionHistoryStore::new(
        SafeStorageRoot::open(&root).expect("storage root"),
        project_key(),
        30,
        Some(port),
    );
    assert_eq!(
        store.list(10).await.unwrap(),
        vec![legacy_record.safe_summary()]
    );
    assert!(
        segment_path(&root, 10).is_file(),
        "legacy 记录必须按 timestamp 日切导出为 jsonl"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// legacy 迁移 fail-open：member 损坏时 warn 后继续空视图，NEVER fail-closed。
#[tokio::test]
async fn corrupt_legacy_member_fails_open_to_empty_view() {
    let root = unique_root("legacy-corrupt");
    let port = storage::wire_file_system_dataset(&root).expect("dataset port");
    seed_legacy_bytes(&port, br#"{"raw_prompt":"must not be accepted"}"#).await;

    let store = JsonlReflectionHistoryStore::new(
        SafeStorageRoot::open(&root).expect("storage root"),
        project_key(),
        30,
        Some(port),
    );
    assert!(store.list(10).await.unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

/// TDD #4：GC 删除超期日切 segment；`retention_days == 0` 禁用 GC。
#[tokio::test]
async fn gc_deletes_expired_history_segments_and_zero_disables() {
    let root = unique_root("gc");
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs();
    let expired_secs = now_secs - 90 * 86_400;
    let store = test_store(&root, 30);
    store
        .append(&record("expired", expired_secs))
        .await
        .unwrap();
    store.append(&record("fresh", now_secs)).await.unwrap();

    let now_unix_ms = now_secs * 1000;
    assert_eq!(store.gc_expired(now_unix_ms, 0).unwrap(), 0, "0 = 禁用 GC");
    assert_eq!(store.list(10).await.unwrap().len(), 2);

    // 配置 retention=30 的免参入口（与 wire 后 composition 触发同路径）。
    assert_eq!(store.gc(now_unix_ms).unwrap(), 1);
    assert_eq!(
        store.list(10).await.unwrap(),
        vec![record("fresh", now_secs).safe_summary()]
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// jsonl 行损坏 = 当前数据损坏 → fail closed（与 legacy 迁移 fail-open 区分）。
#[tokio::test]
async fn corrupt_jsonl_line_fails_closed() {
    let root = unique_root("jsonl-corrupt");
    let store = test_store(&root, 30);
    store.append(&record("valid", 40)).await.unwrap();

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(segment_path(&root, 40))
        .expect("segment open");
    file.write_all(b"not json\n").expect("write corrupt line");
    drop(file);

    let error = store.list(10).await.unwrap_err();
    assert_eq!(
        error,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::Serialization
        }
    );
    std::fs::remove_dir_all(root).unwrap();
}
