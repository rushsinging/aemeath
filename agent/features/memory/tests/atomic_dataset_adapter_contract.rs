use async_trait::async_trait;
use memory::{
    AtomicDatasetMemoryStore, LegacyMemoryLayer, LegacyMemoryMember, LegacyMemorySource,
    LegacyMemorySourceError, MemoryCategory, MemoryCommitVisibility, MemoryDataset,
    MemoryDatasetStore, MemoryEntry, MemoryId, MemoryLayer, MemoryOpenerError, MemoryPolicy,
    MemoryPort, MemorySource, MemoryStorageErrorKind, ProjectMemoryKey, ProjectMemoryOpener,
};
use std::sync::Arc;
use storage::api as storage_api;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-memory-dataset-{case}-{}",
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

fn project_key() -> ProjectMemoryKey {
    ProjectMemoryKey::derive("/contract/project", None).unwrap()
}

fn shared_storage(root: &std::path::Path) -> Arc<dyn storage_api::AtomicDatasetPort> {
    Arc::new(storage::FileSystemDatasetAdapter::new(root).unwrap())
}

fn store(root: &std::path::Path) -> AtomicDatasetMemoryStore {
    AtomicDatasetMemoryStore::new(shared_storage(root), project_key())
}

#[test]
fn storage_error_acl_maps_only_memory_owned_error_kinds() {
    let cases = [
        (
            storage_api::StorageErrorKind::PermissionDenied,
            MemoryStorageErrorKind::PermissionDenied,
        ),
        (
            storage_api::StorageErrorKind::ConcurrentWrite,
            MemoryStorageErrorKind::ConcurrentWrite,
        ),
        (
            storage_api::StorageErrorKind::Io,
            MemoryStorageErrorKind::Io,
        ),
        (
            storage_api::StorageErrorKind::UnsupportedDurability,
            MemoryStorageErrorKind::Io,
        ),
        (
            storage_api::StorageErrorKind::InvalidKey,
            MemoryStorageErrorKind::Serialization,
        ),
    ];
    for (storage_kind, expected) in cases {
        let error = storage_api::StorageError::new(storage_kind, "redacted by ACL");
        assert_eq!(memory::map_storage_error(&error), expected);
    }
}

#[test]
fn adapter_is_public_memory_owned_store() {
    fn assert_store<T: MemoryDatasetStore<Revision = storage_api::DatasetRevision>>() {}
    assert_store::<AtomicDatasetMemoryStore>();
}

#[tokio::test]
async fn filesystem_reopen_round_trip_keeps_each_layer_generation() {
    let root = unique_root("reopen");
    let first = store(&root);
    let empty_global = first.load_committed(MemoryLayer::Global).await.unwrap();
    let empty_project = first.load_committed(MemoryLayer::Project).await.unwrap();
    assert!(empty_global.dataset.active().is_empty());
    assert!(empty_project.dataset.active().is_empty());

    let global = MemoryDataset::new(
        MemoryLayer::Global,
        vec![entry(MemoryLayer::Global, "global decision")],
        vec![entry(MemoryLayer::Global, "global archive")],
    )
    .unwrap();
    let project = MemoryDataset::new(
        MemoryLayer::Project,
        vec![entry(MemoryLayer::Project, "project decision")],
        vec![entry(MemoryLayer::Project, "project archive")],
    )
    .unwrap();

    let global_receipt = first
        .commit(MemoryLayer::Global, &empty_global.revision, &global)
        .await
        .unwrap();
    let project_receipt = first
        .commit(MemoryLayer::Project, &empty_project.revision, &project)
        .await
        .unwrap();
    assert_eq!(global_receipt.visibility(), MemoryCommitVisibility::Visible);
    assert_eq!(
        project_receipt.visibility(),
        MemoryCommitVisibility::Visible
    );
    // Each layer owns an independent generation and therefore a distinct CAS
    // revision.
    assert_ne!(global_receipt.revision(), project_receipt.revision());
    drop(first);

    let reopened = store(&root);
    let loaded_global = reopened.load_committed(MemoryLayer::Global).await.unwrap();
    let loaded_project = reopened.load_committed(MemoryLayer::Project).await.unwrap();
    assert_eq!(loaded_global.dataset, global);
    assert_eq!(loaded_project.dataset, project);
    assert_eq!(&loaded_global.revision, global_receipt.revision());
    assert_eq!(&loaded_project.revision, project_receipt.revision());

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn stale_revision_rejects_layer_generation() {
    let root = unique_root("stale-cas");
    let store = store(&root);
    let stale = store
        .load_committed(MemoryLayer::Project)
        .await
        .unwrap()
        .revision;
    let first_project = MemoryDataset::new(
        MemoryLayer::Project,
        vec![entry(MemoryLayer::Project, "first")],
        vec![],
    )
    .unwrap();
    store
        .commit(MemoryLayer::Project, &stale, &first_project)
        .await
        .unwrap();

    let rejected_project = MemoryDataset::new(
        MemoryLayer::Project,
        vec![entry(MemoryLayer::Project, "must not publish")],
        vec![],
    )
    .unwrap();
    let error = store
        .commit(MemoryLayer::Project, &stale, &rejected_project)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        memory::MemoryError::Storage {
            kind: MemoryStorageErrorKind::ConcurrentWrite
        }
    );
    assert_eq!(
        store
            .load_committed(MemoryLayer::Project)
            .await
            .unwrap()
            .dataset,
        first_project
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Clone)]
struct ScriptedLegacy {
    global: LegacyMemoryLayer,
    project: LegacyMemoryLayer,
}

#[async_trait]
impl LegacyMemorySource for ScriptedLegacy {
    async fn probe(
        &self,
        layer: MemoryLayer,
    ) -> Result<LegacyMemoryLayer, LegacyMemorySourceError> {
        Ok(match layer {
            MemoryLayer::Global => self.global.clone(),
            MemoryLayer::Project => self.project.clone(),
        })
    }
}

fn no_legacy() -> LegacyMemoryLayer {
    LegacyMemoryLayer::default()
}

fn legacy_active(entries: &[MemoryEntry]) -> LegacyMemoryLayer {
    LegacyMemoryLayer {
        active: LegacyMemoryMember::Present(serde_json::to_vec(entries).unwrap()),
        archive: LegacyMemoryMember::Missing,
    }
}

#[tokio::test]
async fn opener_rejects_new_and_legacy_key_conflict() {
    let root = unique_root("legacy-conflict");
    let store = store(&root);
    let loaded = store.load_committed(MemoryLayer::Project).await.unwrap();
    store
        .commit(
            MemoryLayer::Project,
            &loaded.revision,
            &MemoryDataset::new(
                MemoryLayer::Project,
                vec![entry(MemoryLayer::Project, "new")],
                vec![],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let source = ScriptedLegacy {
        global: no_legacy(),
        project: legacy_active(&[entry(MemoryLayer::Project, "legacy")]),
    };

    let result = ProjectMemoryOpener::new(store, Arc::new(source))
        .open(MemoryPolicy::default())
        .await;
    assert!(matches!(result, Err(MemoryOpenerError::LegacyKeyConflict)));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_migrates_legacy_active_only_and_reopens_new_dataset() {
    let root = unique_root("legacy-migrate");
    let legacy_entry = entry(MemoryLayer::Project, "legacy active");
    let source = ScriptedLegacy {
        global: no_legacy(),
        project: legacy_active(std::slice::from_ref(&legacy_entry)),
    };
    let service = ProjectMemoryOpener::new(store(&root), Arc::new(source))
        .open(MemoryPolicy::default())
        .await
        .unwrap();
    assert_eq!(
        service.list(Some(MemoryLayer::Project)),
        vec![legacy_entry.clone()]
    );
    drop(service);

    let reopened = ProjectMemoryOpener::new(
        store(&root),
        Arc::new(ScriptedLegacy {
            global: no_legacy(),
            project: no_legacy(),
        }),
    )
    .open(MemoryPolicy::default())
    .await
    .unwrap();
    assert_eq!(
        reopened.list(Some(MemoryLayer::Project)),
        vec![legacy_entry]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn opener_fails_closed_on_corrupt_legacy_bytes() {
    let root = unique_root("legacy-corrupt");
    let source = ScriptedLegacy {
        global: no_legacy(),
        project: LegacyMemoryLayer {
            active: LegacyMemoryMember::Present(b"not-json".to_vec()),
            archive: LegacyMemoryMember::Missing,
        },
    };
    let result = ProjectMemoryOpener::new(store(&root), Arc::new(source))
        .open(MemoryPolicy::default())
        .await;
    assert!(matches!(result, Err(MemoryOpenerError::CorruptDataset)));
    let loaded = store(&root)
        .load_committed(MemoryLayer::Project)
        .await
        .unwrap();
    assert!(loaded.dataset.active().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn distinct_project_keys_share_global_and_isolate_project() {
    let root = unique_root("shared-global");
    let storage = shared_storage(&root);
    let project_a = ProjectMemoryKey::derive("/project/a", None).unwrap();
    let project_b = ProjectMemoryKey::derive("/project/b", None).unwrap();
    assert_ne!(project_a, project_b);
    let store_a = AtomicDatasetMemoryStore::new(Arc::clone(&storage), project_a);
    let store_b = AtomicDatasetMemoryStore::new(Arc::clone(&storage), project_b);

    // Store A publishes a shared global generation and its own project one.
    let global = MemoryDataset::new(
        MemoryLayer::Global,
        vec![entry(MemoryLayer::Global, "shared global")],
        vec![],
    )
    .unwrap();
    let global_revision = store_a
        .load_committed(MemoryLayer::Global)
        .await
        .unwrap()
        .revision;
    let global_receipt = store_a
        .commit(MemoryLayer::Global, &global_revision, &global)
        .await
        .unwrap();

    let project_a_data = MemoryDataset::new(
        MemoryLayer::Project,
        vec![entry(MemoryLayer::Project, "project a only")],
        vec![],
    )
    .unwrap();
    let project_a_revision = store_a
        .load_committed(MemoryLayer::Project)
        .await
        .unwrap()
        .revision;
    store_a
        .commit(MemoryLayer::Project, &project_a_revision, &project_a_data)
        .await
        .unwrap();

    // Store B has a different project key: it observes the same global
    // generation...
    let loaded_global = store_b.load_committed(MemoryLayer::Global).await.unwrap();
    assert_eq!(loaded_global.dataset, global);
    assert_eq!(&loaded_global.revision, global_receipt.revision());
    // ...yet its project generation is fully isolated and remains empty.
    let loaded_project = store_b.load_committed(MemoryLayer::Project).await.unwrap();
    assert!(loaded_project.dataset.active().is_empty());
    assert!(loaded_project.dataset.archive().is_empty());

    std::fs::remove_dir_all(root).unwrap();
}
