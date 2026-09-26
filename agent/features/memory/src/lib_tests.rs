//! crate 根 wire 工厂契约测试：工厂返回的对象满足 trait 且可完成最小操作。
use crate::api::{
    LegacyMemoryLayer, LegacyMemoryMember, LegacyMemorySourceFactory, MemoryLayer, MemoryOpener,
    ReflectionHistoryStore, ReflectionRecord, ReflectionTrigger,
};
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
        storage::file_system_dataset(&root).unwrap(),
        crate::wire_legacy_memory_source_factory(root.join("legacy")),
    );
    let port = opener
        .open_memory(&project_key(), &share::config::MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port.stats().project_count, 0);

    // `Box<dyn MemoryOpener>: Clone` 经 `boxed_clone` 生效。
    let cloned = opener.clone();
    let port2 = cloned
        .open_memory(&project_key(), &share::config::MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port2.stats().project_count, 0);

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
        storage::file_system_dataset(&root).unwrap(),
        project_key(),
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
