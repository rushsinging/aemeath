//! Contract tests for the Memory-owned production [`LegacyMemorySourceFactory`]
//! and the [`FileLegacyMemorySource`] it creates.
//!
//! Verifies:
//! 1. **missing → Missing** — no legacy files means every member is Missing.
//! 2. **active/archive single-side reads** — only the active (or only the
//!    archive) file present yields Present/Missing respectively.
//! 3. **permission/io classification** — EACCES maps to PermissionDenied,
//!    other I/O failures map to Io.
//! 4. **different projects don't cross-contaminate** — project A's legacy
//!    files are invisible when probing project B's source.
//! 5. **factory is cloneable** — `Box<dyn LegacyMemorySourceFactory>: Clone`.
//! 6. **end-to-end migration** — the opener reads legacy files, migrates them
//!    to the new dataset format, and exposes the entries through the port.

use memory::{
    DatasetMemoryOpener, FileLegacyMemorySourceFactory, LegacyMemoryMember, LegacyMemorySource,
    LegacyMemorySourceError, LegacyMemorySourceFactory, MemoryCategory, MemoryEntry, MemoryId,
    MemoryLayer, MemoryOpener, MemorySource, ProjectMemoryKey,
};
use share::config::MemoryConfig;
use std::sync::Arc;
use storage::api as storage_api;

// ── helpers ─────────────────────────────────────────────────────────────

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-memory-legacy-source-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn key(cwd: &str) -> ProjectMemoryKey {
    ProjectMemoryKey::derive(cwd, None).unwrap()
}

/// Compute the legacy file stem the same way `ProjectMemoryKey` does
/// internally — stripping leading `/` and replacing remaining `/` with `-`.
/// Tests need this to construct legacy files at the exact positions the
/// production source expects.
fn legacy_stem(cwd: &str) -> String {
    cwd.trim_start_matches('/').replace('/', "-")
}

fn storage(root: &std::path::Path) -> Arc<dyn storage_api::AtomicDatasetPort> {
    Arc::new(storage::FileSystemDatasetAdapter::new(root).unwrap())
}

/// Serialize a single legacy entry as the plain JSON array written by the
/// predecessor flat-file format.
fn legacy_entry_bytes(content: &str, layer: MemoryLayer) -> Vec<u8> {
    let entry = MemoryEntry::new(
        MemoryId::now_v7(),
        42,
        layer,
        MemoryCategory::Decision,
        content,
        MemorySource::User,
    )
    .unwrap();
    serde_json::to_vec(&[entry]).unwrap()
}

fn write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

// ── 1. missing → Missing ───────────────────────────────────────────────

#[tokio::test]
async fn missing_files_return_all_missing() {
    let root = unique_root("missing");
    std::fs::create_dir_all(&root).unwrap();

    let factory = FileLegacyMemorySourceFactory::new(&root);
    let source = factory.create_for(&key("/missing/project"));

    let global = source.probe(MemoryLayer::Global).await.unwrap();
    assert_eq!(global.active, LegacyMemoryMember::Missing);
    assert_eq!(global.archive, LegacyMemoryMember::Missing);
    assert!(!global.is_present());

    let project = source.probe(MemoryLayer::Project).await.unwrap();
    assert_eq!(project.active, LegacyMemoryMember::Missing);
    assert_eq!(project.archive, LegacyMemoryMember::Missing);
    assert!(!project.is_present());

    std::fs::remove_dir_all(root).unwrap();
}

// ── 2. active/archive single-side reads ─────────────────────────────────

#[tokio::test]
async fn active_only_present_archive_missing() {
    let root = unique_root("active-only");
    let project = key("/active/only");

    write_file(
        &root,
        &format!("{}.json", legacy_stem("/active/only")),
        &legacy_entry_bytes("active only fact", MemoryLayer::Project),
    );
    // No _archive file.

    let factory = FileLegacyMemorySourceFactory::new(&root);
    let source = factory.create_for(&project);

    let layer = source.probe(MemoryLayer::Project).await.unwrap();
    assert!(matches!(layer.active, LegacyMemoryMember::Present(_)));
    assert_eq!(layer.archive, LegacyMemoryMember::Missing);
    assert!(layer.is_present());

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn archive_only_present_active_missing() {
    let root = unique_root("archive-only");
    let project = key("/archive/only");

    write_file(
        &root,
        &format!("{}_archive.json", legacy_stem("/archive/only")),
        &legacy_entry_bytes("archived fact", MemoryLayer::Project),
    );

    let factory = FileLegacyMemorySourceFactory::new(&root);
    let source = factory.create_for(&project);

    let layer = source.probe(MemoryLayer::Project).await.unwrap();
    assert_eq!(layer.active, LegacyMemoryMember::Missing);
    assert!(matches!(layer.archive, LegacyMemoryMember::Present(_)));
    assert!(layer.is_present());

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn global_files_read_independently_of_project_files() {
    let root = unique_root("global");

    write_file(
        &root,
        "_global.json",
        &legacy_entry_bytes("global active", MemoryLayer::Global),
    );
    write_file(
        &root,
        "_global_archive.json",
        &legacy_entry_bytes("global archived", MemoryLayer::Global),
    );

    let factory = FileLegacyMemorySourceFactory::new(&root);
    // Project files don't exist, but global do.
    let source = factory.create_for(&key("/global/test"));

    let global = source.probe(MemoryLayer::Global).await.unwrap();
    assert!(matches!(global.active, LegacyMemoryMember::Present(_)));
    assert!(matches!(global.archive, LegacyMemoryMember::Present(_)));

    let project = source.probe(MemoryLayer::Project).await.unwrap();
    assert_eq!(project.active, LegacyMemoryMember::Missing);
    assert_eq!(project.archive, LegacyMemoryMember::Missing);

    std::fs::remove_dir_all(root).unwrap();
}

// ── 3. permission / io classification ───────────────────────────────────

#[cfg(unix)]
#[tokio::test]
async fn permission_denied_is_classified() {
    use std::os::unix::fs::PermissionsExt;

    let root = unique_root("perm");
    let project = key("/perm/test");
    let dir = root.join("perm-denied");
    std::fs::create_dir_all(&dir).unwrap();

    // Write a project active file, then strip read permission from the file.
    let file_name = format!("{}.json", legacy_stem("/perm/test"));
    write_file(
        &dir,
        &file_name,
        &legacy_entry_bytes("unreadable", MemoryLayer::Project),
    );
    let file_path = dir.join(&file_name);
    let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&file_path, perms).unwrap();

    // Root (uid 0) bypasses file permission checks; if we can still read the
    // file, the test environment is root and we cannot verify EACCES.
    if std::fs::read(&file_path).is_ok() {
        eprintln!("skipping permission test: running as root");
        std::fs::remove_dir_all(root).unwrap();
        return;
    }

    let factory = FileLegacyMemorySourceFactory::new(&dir);
    let source = factory.create_for(&project);

    let result = source.probe(MemoryLayer::Project).await;
    assert_eq!(
        result.unwrap_err(),
        LegacyMemorySourceError::PermissionDenied
    );

    // Restore so cleanup works.
    let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&file_path, perms).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn directory_in_place_of_file_is_io_error() {
    let root = unique_root("io");
    let project = key("/io/test");
    std::fs::create_dir_all(&root).unwrap();

    // Create a *directory* where the active file should be — reading it
    // succeeds on open but fails on read, yielding a non-PermissionDenied I/O.
    let file_name = format!("{}.json", legacy_stem("/io/test"));
    std::fs::create_dir_all(root.join(&file_name)).unwrap();

    let factory = FileLegacyMemorySourceFactory::new(&root);
    let source = factory.create_for(&project);

    let result = source.probe(MemoryLayer::Project).await;
    assert_eq!(result.unwrap_err(), LegacyMemorySourceError::Io);

    std::fs::remove_dir_all(root).unwrap();
}

// ── 4. different projects don't cross-contaminate ───────────────────────

#[tokio::test]
async fn distinct_projects_have_distinct_legacy_files() {
    let root = unique_root("isolation");
    let key_a = key("/iso/project-a");
    let key_b = key("/iso/project-b");

    write_file(
        &root,
        &format!("{}.json", legacy_stem("/iso/project-a")),
        &legacy_entry_bytes("project A only", MemoryLayer::Project),
    );
    write_file(
        &root,
        &format!("{}.json", legacy_stem("/iso/project-b")),
        &legacy_entry_bytes("project B only", MemoryLayer::Project),
    );

    let factory = FileLegacyMemorySourceFactory::new(&root);

    let layer_a = factory
        .create_for(&key_a)
        .probe(MemoryLayer::Project)
        .await
        .unwrap();
    let layer_b = factory
        .create_for(&key_b)
        .probe(MemoryLayer::Project)
        .await
        .unwrap();

    // Each project sees only its own legacy bytes.
    let bytes_a = match &layer_a.active {
        LegacyMemoryMember::Present(b) => b.clone(),
        _ => panic!("project A active should be Present"),
    };
    let bytes_b = match &layer_b.active {
        LegacyMemoryMember::Present(b) => b.clone(),
        _ => panic!("project B active should be Present"),
    };

    let a_entries: Vec<MemoryEntry> = serde_json::from_slice(&bytes_a).unwrap();
    let b_entries: Vec<MemoryEntry> = serde_json::from_slice(&bytes_b).unwrap();
    assert_eq!(a_entries[0].content, "project A only");
    assert_eq!(b_entries[0].content, "project B only");

    // Cross-check: A's archive is missing for both.
    assert_eq!(layer_a.archive, LegacyMemoryMember::Missing);
    assert_eq!(layer_b.archive, LegacyMemoryMember::Missing);

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn project_a_legacy_invisible_to_project_b_source() {
    let root = unique_root("cross");
    let key_b = key("/cross/b");

    // Only project A has legacy files.
    write_file(
        &root,
        &format!("{}.json", legacy_stem("/cross/a")),
        &legacy_entry_bytes("A exclusive", MemoryLayer::Project),
    );

    let factory = FileLegacyMemorySourceFactory::new(&root);

    let layer_b = factory
        .create_for(&key_b)
        .probe(MemoryLayer::Project)
        .await
        .unwrap();
    assert_eq!(layer_b.active, LegacyMemoryMember::Missing);
    assert!(!layer_b.is_present());

    std::fs::remove_dir_all(root).unwrap();
}

// ── 5. factory cloneability ─────────────────────────────────────────────

#[tokio::test]
async fn factory_is_object_safe_and_cloneable() {
    let root = unique_root("factory-dyn");
    std::fs::create_dir_all(&root).unwrap();
    let factory: Box<dyn LegacyMemorySourceFactory> =
        Box::new(FileLegacyMemorySourceFactory::new(&root));

    let source = factory.create_for(&key("/dyn/test"));
    let layer = source.probe(MemoryLayer::Project).await.unwrap();
    assert_eq!(layer.active, LegacyMemoryMember::Missing);

    // Cloneable via Box<dyn ...>: Clone.
    let cloned = factory.clone();
    let source2 = cloned.create_for(&key("/dyn/test"));
    let layer2 = source2.probe(MemoryLayer::Project).await.unwrap();
    assert_eq!(layer2.active, LegacyMemoryMember::Missing);

    std::fs::remove_dir_all(root).unwrap();
}

// ── 6. end-to-end migration through the opener ──────────────────────────

#[tokio::test]
async fn opener_migrates_legacy_global_and_project() {
    let root = unique_root("e2e");
    let legacy_dir = root.join("legacy");
    let storage_dir = root.join("storage");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&storage_dir).unwrap();

    let project = key("/e2e/project");

    // Write legacy files for both layers.
    write_file(
        &legacy_dir,
        "_global.json",
        &legacy_entry_bytes("legacy global fact", MemoryLayer::Global),
    );
    write_file(
        &legacy_dir,
        &format!("{}.json", legacy_stem("/e2e/project")),
        &legacy_entry_bytes("legacy project fact", MemoryLayer::Project),
    );

    let opener = DatasetMemoryOpener::new(
        storage(&storage_dir),
        Arc::new(FileLegacyMemorySourceFactory::new(&legacy_dir)),
    );

    let port = opener
        .open_memory(&project, &MemoryConfig::default())
        .await
        .unwrap();

    // Migration should have brought the legacy entries into the new dataset.
    let global = port.list(Some(MemoryLayer::Global));
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].content, "legacy global fact");

    let project_entries = port.list(Some(MemoryLayer::Project));
    assert_eq!(project_entries.len(), 1);
    assert_eq!(project_entries[0].content, "legacy project fact");

    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn opener_legacy_conflict_when_new_data_exists() {
    use memory::MemoryOpenerError;

    let root = unique_root("conflict");
    let legacy_dir = root.join("legacy");
    let storage_dir = root.join("storage");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&storage_dir).unwrap();

    let project = key("/conflict/project");

    // First open with no legacy → creates a new dataset with one entry.
    {
        #[derive(Clone)]
        struct NoLegacy;
        #[async_trait::async_trait]
        impl LegacyMemorySource for NoLegacy {
            async fn probe(
                &self,
                _layer: MemoryLayer,
            ) -> Result<memory::LegacyMemoryLayer, LegacyMemorySourceError> {
                Ok(memory::LegacyMemoryLayer::default())
            }
        }
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
        let opener = DatasetMemoryOpener::new(storage(&storage_dir), Arc::new(NoLegacyFactory));
        let port = opener
            .open_memory(&project, &MemoryConfig::default())
            .await
            .unwrap();
        port.write(
            MemoryEntry::new(
                MemoryId::now_v7(),
                1,
                MemoryLayer::Project,
                MemoryCategory::Fact,
                "new data",
                MemorySource::User,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }

    // Second open: now legacy files exist AND new data exists → conflict.
    write_file(
        &legacy_dir,
        &format!("{}.json", legacy_stem("/conflict/project")),
        &legacy_entry_bytes("legacy fact", MemoryLayer::Project),
    );

    let opener = DatasetMemoryOpener::new(
        storage(&storage_dir),
        Arc::new(FileLegacyMemorySourceFactory::new(&legacy_dir)),
    );

    let result = opener.open_memory(&project, &MemoryConfig::default()).await;
    assert!(matches!(result, Err(MemoryOpenerError::LegacyKeyConflict)));

    std::fs::remove_dir_all(&root).unwrap();
}
