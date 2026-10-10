use memory::api::reflection::{ReflectionErrorCategory, ReflectionRecord, ReflectionTrigger};
use memory::api::{MemoryError, MemoryStorageErrorKind, ProjectMemoryKey, ReflectionHistoryStore};
use std::sync::Arc;
use storage as storage_api;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-reflection-history-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn project_key() -> ProjectMemoryKey {
    ProjectMemoryKey::derive("/reflection/history/contract", None).unwrap()
}

fn storage(root: &std::path::Path) -> Arc<dyn storage_api::AtomicDatasetPort> {
    storage::wire_file_system_dataset(root).unwrap()
}

/// 实现体已收窄 crate 内：跨 crate 构造只能经 `wire_reflection_history_store`。
/// jsonl 根与 dataset 根同为 `root`（`SafeStorageRoot::open`），legacy dataset
/// 作为一次性导出源注入。
fn store(root: &std::path::Path) -> Arc<dyn ReflectionHistoryStore> {
    memory::wire_reflection_history_store(
        storage::SafeStorageRoot::open(root).unwrap(),
        project_key(),
        30,
        Some(storage(root)),
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

#[tokio::test]
async fn reflection_history_upsert_folds_stable_id_to_latest_on_read() {
    let root = unique_root("upsert");
    let history = store(&root);
    let running =
        memory::api::reflection::ReflectionRecord::running("stable", 40, ReflectionTrigger::Manual);
    history.append(&running).await.unwrap();
    let terminal = record("stable", 40);
    history.upsert(&terminal).await.unwrap();

    assert_eq!(
        history.list(10).await.unwrap(),
        vec![terminal.safe_summary()]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_append_and_list_round_trip() {
    let root = unique_root("append-list");
    let history = store(&root);
    let first = record("first", 10);
    let second = record("second", 20);

    history.append(&first).await.unwrap();
    history.append(&second).await.unwrap();

    assert_eq!(
        history.list(10).await.unwrap(),
        vec![second.safe_summary(), first.safe_summary()]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_reopen_keeps_records() {
    let root = unique_root("reopen");
    // 当前时间戳：GC-on-wire 只删超出 retention 窗口的日切段，
    // 当日 segment 必须在 reopen 后幸存（1970 日切会被 GC 正确回收）。
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs();
    let expected = record("durable", now_secs);
    store(&root).append(&expected).await.unwrap();

    let reopened = store(&root);
    assert_eq!(
        reopened.list(10).await.unwrap(),
        vec![expected.safe_summary()]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_limit_returns_newest_records_only() {
    let root = unique_root("limit");
    let history = store(&root);
    for (id, timestamp) in [("one", 1), ("two", 2), ("three", 3)] {
        history.append(&record(id, timestamp)).await.unwrap();
    }

    assert_eq!(
        history
            .list(2)
            .await
            .unwrap()
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        vec!["three", "two"]
    );
    assert!(history.list(0).await.unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

/// jsonl 行损坏 = 当前数据损坏 → fail closed（legacy member 损坏则 fail-open，见 crate 内单测）。
#[tokio::test]
async fn reflection_history_corruption_fails_closed() {
    let root = unique_root("corruption");
    let history = store(&root);
    history.append(&record("valid", 40)).await.unwrap();

    // timestamp=40 → UTC 日 1970-01-01；在当日 segment 末尾追加非法行。
    let segment = root
        .join("memory")
        .join(project_key().as_str())
        .join("reflection-history")
        .join("1970-01-01.jsonl");
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&segment)
            .expect("segment open");
        file.write_all(b"not json\n").expect("write corrupt line");
    }

    let error = history.list(10).await.unwrap_err();
    assert_eq!(
        error,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::Serialization
        }
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn reflection_history_adapter_is_memory_owned_port() {
    // 具体实现体已收窄 crate 内；此处由 wire 工厂的 trait 对象消费面守住
    // ReflectionHistoryStore 约束。
    fn assert_store<T: ReflectionHistoryStore + ?Sized>() {}
    assert_store::<dyn ReflectionHistoryStore>();
}
