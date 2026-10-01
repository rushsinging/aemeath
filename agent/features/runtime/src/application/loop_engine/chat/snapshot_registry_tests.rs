use super::*;
use std::fs;
use std::io::Write;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aemeath_snapshot_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    let mut f = fs::File::create(&path).unwrap();
    write!(f, "{}", content).unwrap();
    path
}

#[test]
fn test_no_change_detected() {
    let dir = temp_dir();
    let path = write_file(&dir, "test_no_change.txt", "hello");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    let changes = registry.check_for_changes();
    assert!(changes.is_empty(), "expected no changes, got {:?}", changes);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_modification_detected() {
    let dir = temp_dir();
    let path = write_file(&dir, "test_mod.txt", "hello");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&path, "world").unwrap();

    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, path);
    assert_eq!(changes[0].kind, FileChangeKind::Modified);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_deletion_detected() {
    let dir = temp_dir();
    let path = write_file(&dir, "test_del.txt", "hello");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    fs::remove_file(&path).unwrap();

    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, path);
    assert_eq!(changes[0].kind, FileChangeKind::Deleted);

    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_addition_detected() {
    let dir = temp_dir();
    let path = dir.join("test_add.txt");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    write_file(&dir, "test_add.txt", "new content");

    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, path);
    assert_eq!(changes[0].kind, FileChangeKind::Added);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_no_change_after_update() {
    let dir = temp_dir();
    let path = write_file(&dir, "test_update.txt", "hello");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&path, "world").unwrap();
    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, FileChangeKind::Modified);

    let changes = registry.check_for_changes();
    assert!(
        changes.is_empty(),
        "expected no changes after update, got {:?}",
        changes
    );

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_empty_file_no_change() {
    let dir = temp_dir();
    let path = write_file(&dir, "test_empty.txt", "");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    let changes = registry.check_for_changes();
    assert!(changes.is_empty());

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_nonexistent_file_no_snapshot() {
    let dir = temp_dir();
    let path = dir.join("nonexistent.txt");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(path.clone());
    registry.take_baseline();

    assert!(registry.snapshots.is_empty());

    let _ = fs::remove_dir(&dir);
}
