//! crate 根 wire 工厂契约测试：工厂返回的对象满足 trait 且可完成最小操作。
use crate::api::legacy::{LegacyMemoryLayer, LegacyMemoryMember, LegacyMemorySourceFactory};
use crate::api::reflection::{ReflectionRecord, ReflectionTrigger};
use crate::api::{MemoryLayer, MemoryOpener, ReflectionHistoryStore};
use crate::domain::ProjectMemoryKey;
use std::sync::Arc;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-memory-wire-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn project_key() -> ProjectMemoryKey {
    ProjectMemoryKey::derive("/wire/factory", None).unwrap()
}

/// `wire_memory_opener` 返回 `Box<dyn MemoryOpener>`：对象安全、可克隆，
/// 能完成 open → 拿到可用 `MemoryPort` 的最小操作。
#[tokio::test]
async fn wire_memory_opener_returns_object_safe_cloneable_opener() {
    let root = unique_root("opener");
    std::fs::create_dir_all(&root).unwrap();

    let opener: Box<dyn MemoryOpener> = crate::wire_memory_opener(
        storage::wire_file_system_dataset(&root).unwrap(),
        crate::wire_legacy_memory_source_factory(root.join("legacy")),
        None,
        storage::SafeStorageRoot::open(&root).unwrap(),
    );
    let port = opener
        .open_memory(&project_key(), &share::config::MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port.stats().await.project_count, 0);

    // `Box<dyn MemoryOpener>: Clone` 经 `boxed_clone` 生效。
    let cloned = opener.clone();
    let port2 = cloned
        .open_memory(&project_key(), &share::config::MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port2.stats().await.project_count, 0);

    std::fs::remove_dir_all(root).unwrap();
}

/// `wire_legacy_memory_source_factory` 返回 `Arc<dyn LegacyMemorySourceFactory>`：
/// 能 create_for → probe，缺文件按契约映射 `Missing`。
#[tokio::test]
async fn wire_legacy_memory_source_factory_returns_probe_capable_factory() {
    let root = unique_root("legacy");
    std::fs::create_dir_all(&root).unwrap();

    let factory: Arc<dyn LegacyMemorySourceFactory> =
        crate::wire_legacy_memory_source_factory(&root);
    let source = factory.create_for(&project_key());
    let layer: LegacyMemoryLayer = source.probe(MemoryLayer::Global).await.unwrap();
    assert_eq!(layer.active, LegacyMemoryMember::Missing);
    assert_eq!(layer.archive, LegacyMemoryMember::Missing);

    std::fs::remove_dir_all(root).unwrap();
}

/// `wire_reflection_history_store` 返回 `Arc<dyn ReflectionHistoryStore>`：
/// 能 append → list 完成最小读写闭环。
#[tokio::test]
async fn wire_reflection_history_store_appends_and_lists() {
    let root = unique_root("history");
    std::fs::create_dir_all(&root).unwrap();

    let store: Arc<dyn ReflectionHistoryStore> = crate::wire_reflection_history_store(
        storage::SafeStorageRoot::open(&root).unwrap(),
        project_key(),
        30,
        Some(storage::wire_file_system_dataset(&root).unwrap()),
    );
    store
        .append(&ReflectionRecord::running(
            "wire",
            1,
            ReflectionTrigger::Manual,
        ))
        .await
        .unwrap();
    assert_eq!(store.list(10).await.unwrap().len(), 1);

    std::fs::remove_dir_all(root).unwrap();
}

/// `list_with_content` 投影偏差文本与建议内容；`list` 默认摘要不携带（Safe 边界）。
#[tokio::test]
async fn reflection_history_list_with_content_projects_texts_and_suggestions() {
    let root = unique_root("history-content");
    std::fs::create_dir_all(&root).unwrap();

    let store: Arc<dyn ReflectionHistoryStore> = crate::wire_reflection_history_store(
        storage::SafeStorageRoot::open(&root).unwrap(),
        project_key(),
        30,
        Some(storage::wire_file_system_dataset(&root).unwrap()),
    );
    let mut record = ReflectionRecord::running("content-1", 7, ReflectionTrigger::Manual);
    record.output = Some(crate::domain::ReflectionOutput {
        deviations: vec!["deviation text".into()],
        suggested_memories: vec![crate::domain::MemorySuggestion {
            layer: crate::domain::MemoryLayer::Project,
            category: crate::domain::MemoryCategory::Fact,
            content: "suggestion content".into(),
            tags: vec![],
            reason: "because".into(),
            supersedes: vec![],
            synthesizes: Vec::new(),
        }],
        outdated_memories: vec![],
    });
    record.status = crate::domain::ReflectionStatus::Succeeded;
    store.upsert(&record).await.unwrap();

    let plain = store.list(10).await.unwrap();
    assert_eq!(plain.len(), 1);
    assert!(plain[0].deviation_texts.is_none());
    assert!(plain[0].suggested_memories.is_none());

    let with_content = store.list_with_content(10).await.unwrap();
    assert_eq!(with_content.len(), 1);
    assert_eq!(
        with_content[0].deviation_texts.as_deref(),
        Some(&["deviation text".to_string()][..])
    );
    let suggestions = with_content[0]
        .suggested_memories
        .as_ref()
        .expect("内容投影必须携带建议");
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].content, "suggestion content");

    std::fs::remove_dir_all(root).unwrap();
}

/// 生产 opener（`wire_memory_opener` + 真 event root）打开的 Memory 把事件写进
/// 真实 `memory/{project}/events/yyyy-mm-dd.jsonl`——证明生产路径不再落 Noop。
#[tokio::test]
async fn wire_memory_opener_open_memory_appends_events_to_daily_jsonl() {
    use crate::api::{MemoryCategory, MemoryEntry, MemoryId, MemorySource};
    use crate::domain::event::MemoryEventOp;

    let root = unique_root("opener-events");
    std::fs::create_dir_all(&root).unwrap();
    let key = project_key();

    let opener: Box<dyn MemoryOpener> = crate::wire_memory_opener(
        storage::wire_file_system_dataset(&root).unwrap(),
        crate::wire_legacy_memory_source_factory(root.join("legacy")),
        None,
        storage::SafeStorageRoot::open(&root).unwrap(),
    );
    let port = opener
        .open_memory(&key, &share::config::MemoryConfig::default())
        .await
        .unwrap();

    // 写入触发 WriteAdd（emit fail-open，成功写入必达真 store）。
    let entry = MemoryEntry::new(
        MemoryId::now_v7(),
        42,
        MemoryLayer::Project,
        MemoryCategory::Decision,
        "event wiring fact",
        MemorySource::User,
    )
    .unwrap();
    port.write(entry).await.unwrap();

    // 断言日切 jsonl 落盘于 memory/{project}/events/yyyy-mm-dd.jsonl。
    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let events_file = root
        .join("memory")
        .join(key.as_str())
        .join("events")
        .join(format!("{day}.jsonl"));
    assert!(
        events_file.is_file(),
        "事件日切文件必须落盘：{}",
        events_file.display()
    );

    // 读回内容确认确为 WriteAdd 事件（空文件不算数）。
    let store = crate::event_jsonl::JsonlSegmentEventStore::new(
        storage::SafeStorageRoot::open(&root).unwrap(),
        key.clone(),
        30,
    );
    let events = store.read_all_for_test().expect("读回事件");
    assert!(
        events
            .iter()
            .any(|event| matches!(event.op, MemoryEventOp::WriteAdd)),
        "事件流必须包含 WriteAdd"
    );

    std::fs::remove_dir_all(root).unwrap();
}
