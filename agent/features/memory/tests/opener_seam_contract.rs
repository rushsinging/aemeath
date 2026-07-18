//! Task #10 contract tests for the project-aware, object-safe, cloneable
//! Memory opener seam.
//!
//! The seam accepts a Project-owned identity (`ProjectMemoryKey`) and a
//! candidate `share::config::MemoryConfig`, eagerly opens both layers, and
//! returns `Arc<dyn MemoryPort>`. Memory never imports the Config service or
//! reads the current config — the candidate config is supplied by the caller.

use async_trait::async_trait;
use memory::{
    DatasetMemoryOpener, LegacyMemoryLayer, LegacyMemorySource, LegacyMemorySourceError,
    LegacyMemorySourceFactory, MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemoryOpener,
    MemoryPort, MemorySource, ProjectMemoryKey,
};
use share::config::MemoryConfig;
use std::sync::Arc;
use storage::api as storage_api;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-memory-opener-seam-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        42,
        layer,
        MemoryCategory::Decision,
        content,
        MemorySource::User,
    )
    .unwrap()
}

fn key(path: &str) -> ProjectMemoryKey {
    ProjectMemoryKey::derive(path, None).unwrap()
}

fn storage(root: &std::path::Path) -> Arc<dyn storage_api::AtomicDatasetPort> {
    Arc::new(storage::FileSystemDatasetAdapter::new(root).unwrap())
}

/// Legacy source that always reports no legacy members.
#[derive(Clone)]
struct NoLegacy;

#[async_trait]
impl LegacyMemorySource for NoLegacy {
    async fn probe(
        &self,
        _layer: MemoryLayer,
    ) -> Result<LegacyMemoryLayer, LegacyMemorySourceError> {
        Ok(LegacyMemoryLayer::default())
    }
}

/// Factory that always creates sources reporting no legacy members.
#[derive(Clone)]
struct NoLegacyFactory;

impl LegacyMemorySourceFactory for NoLegacyFactory {
    fn create_for(&self, _key: &ProjectMemoryKey) -> Arc<dyn LegacyMemorySource> {
        Arc::new(NoLegacy)
    }

    fn boxed_clone(&self) -> Box<dyn LegacyMemorySourceFactory> {
        Box::new(self.clone())
    }
}

#[tokio::test]
async fn opener_is_object_safe_dyn_dispatchable_and_cloneable() {
    let root = unique_root("dyn");
    let opener: Box<dyn MemoryOpener> = Box::new(DatasetMemoryOpener::new(
        storage(&root),
        Arc::new(NoLegacyFactory),
    ));

    // Object-safe: method is callable behind `dyn MemoryOpener`.
    let port = opener
        .open_memory(&key("/dyn/project"), &MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port.stats().project_count, 0);

    // Cloneable: `Box<dyn MemoryOpener>: Clone` via `boxed_clone`.
    let cloned = opener.clone();
    let port2 = cloned
        .open_memory(&key("/dyn/other"), &MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port2.stats().project_count, 0);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_eagerly_opens_and_returns_usable_memory_port() {
    let root = unique_root("eager");
    let opener = DatasetMemoryOpener::new(storage(&root), Arc::new(NoLegacyFactory));

    let port: Arc<dyn MemoryPort> = opener
        .open_memory(&key("/eager/project"), &MemoryConfig::default())
        .await
        .unwrap();

    // The returned Arc<dyn MemoryPort> is immediately usable.
    let fact = entry(MemoryLayer::Project, "eagerly opened project fact");
    port.write(fact.clone()).await.unwrap();
    assert_eq!(port.list(Some(MemoryLayer::Project)), vec![fact.clone()]);
    assert_eq!(port.stats().project_count, 1);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_config_drives_memory_policy() {
    let root = unique_root("policy");
    let opener = DatasetMemoryOpener::new(storage(&root), Arc::new(NoLegacyFactory));

    let config = MemoryConfig {
        max_entries: 1,
        ..MemoryConfig::default()
    };

    let port = opener
        .open_memory(&key("/policy/project"), &config)
        .await
        .unwrap();

    // With max_entries = 1, the second write should trigger NeedsEviction.
    port.write(entry(MemoryLayer::Project, "first"))
        .await
        .unwrap();
    let result = port
        .write(entry(MemoryLayer::Project, "second"))
        .await
        .unwrap();
    assert!(matches!(result, memory::WriteResult::NeedsEviction { .. }));

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_distinct_project_keys_isolate_project_and_share_global() {
    let root = unique_root("isolation");
    let shared = storage(&root);
    let opener = DatasetMemoryOpener::new(Arc::clone(&shared), Arc::new(NoLegacyFactory));

    let port_a = opener
        .open_memory(&key("/iso/a"), &MemoryConfig::default())
        .await
        .unwrap();
    let port_b = opener
        .open_memory(&key("/iso/b"), &MemoryConfig::default())
        .await
        .unwrap();

    // Write to global via port_a.
    let global_fact = entry(MemoryLayer::Global, "shared global fact");
    port_a.write(global_fact.clone()).await.unwrap();

    // Write to project-a only.
    let project_a_fact = entry(MemoryLayer::Project, "project a only");
    port_a.write(project_a_fact.clone()).await.unwrap();

    // Project isolation: port_b does not see project-a entries.
    assert!(port_b.list(Some(MemoryLayer::Project)).is_empty());
    // Shared global: port_b sees the global generation committed by port_a.
    // (Both ports point at committed generations; port_b loaded the global
    // layer at open time which may be before port_a wrote. So we re-open
    // to verify persistence.)
    drop(port_a);
    drop(port_b);

    let port_b_reopened = opener
        .open_memory(&key("/iso/b"), &MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(
        port_b_reopened.list(Some(MemoryLayer::Global)),
        vec![global_fact]
    );
    // Project-b still isolated.
    assert!(port_b_reopened.list(Some(MemoryLayer::Project)).is_empty());

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_clone_shares_storage_wiring() {
    let root = unique_root("clone-share");
    let opener = DatasetMemoryOpener::new(storage(&root), Arc::new(NoLegacyFactory));
    let cloned = opener.clone();

    // Both the original and the clone open against the same Storage root.
    let port = opener
        .open_memory(&key("/clone/project"), &MemoryConfig::default())
        .await
        .unwrap();
    port.write(entry(MemoryLayer::Project, "original writes"))
        .await
        .unwrap();
    drop(port);

    // The clone sees what the original wrote.
    let port_via_clone = cloned
        .open_memory(&key("/clone/project"), &MemoryConfig::default())
        .await
        .unwrap();
    assert_eq!(port_via_clone.list(Some(MemoryLayer::Project)).len(), 1);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_can_be_used_as_dyn_in_a_collection() {
    let root = unique_root("dyn-collect");
    let openers: Vec<Box<dyn MemoryOpener>> = vec![
        Box::new(DatasetMemoryOpener::new(
            storage(&root),
            Arc::new(NoLegacyFactory),
        )),
        // Cloned entry to exercise `Box<dyn MemoryOpener>: Clone`.
        Box::new(DatasetMemoryOpener::new(
            storage(&root),
            Arc::new(NoLegacyFactory),
        ))
        .clone(),
    ];

    for opener in &openers {
        let port = opener
            .open_memory(&key("/collect/project"), &MemoryConfig::default())
            .await
            .unwrap();
        assert_eq!(port.stats().global_count, 0);
    }

    std::fs::remove_dir_all(root).unwrap();
}
