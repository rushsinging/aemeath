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

// ── 目录监控（#2015 热载 watch 面）──

#[test]
fn directory_snapshot_detects_added_entry() {
    let parent = temp_dir();
    let skills_dir = parent.join("skills");
    fs::create_dir_all(&skills_dir).expect("create skills dir");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(skills_dir.clone());
    registry.take_baseline();
    assert!(registry.check_for_changes().is_empty());

    fs::write(skills_dir.join("alpha.md"), "new skill").expect("write skill");
    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1, "新增子条目必须检出：{changes:?}");
    assert_eq!(changes[0].path, skills_dir);
    assert_eq!(changes[0].kind, FileChangeKind::Modified);

    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn directory_snapshot_detects_removed_entry() {
    let parent = temp_dir();
    let skills_dir = parent.join("skills");
    fs::create_dir_all(&skills_dir).expect("create skills dir");
    fs::write(skills_dir.join("alpha.md"), "skill").expect("write skill");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(skills_dir.clone());
    registry.take_baseline();

    fs::remove_file(skills_dir.join("alpha.md")).expect("remove skill");
    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1, "删除子条目必须检出：{changes:?}");
    assert_eq!(changes[0].kind, FileChangeKind::Modified);

    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn directory_snapshot_detects_child_content_change_without_dir_mtime_change() {
    let parent = temp_dir();
    let skills_dir = parent.join("skills");
    fs::create_dir_all(&skills_dir).expect("create skills dir");
    let child = skills_dir.join("alpha.md");
    fs::write(&child, "v1").expect("write skill");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(skills_dir.clone());
    registry.take_baseline();

    // 子文件内容变化：目录 mtime 与条目数均不变——目录指纹（含子条目 mtime
    // 的清单）MUST 检出，NEVER 因目录元数据未变而跳过。
    fs::write(&child, "v2 with different content").expect("rewrite skill");
    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1, "子文件内容变化必须检出：{changes:?}");
    assert_eq!(changes[0].kind, FileChangeKind::Modified);

    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn directory_snapshot_unchanged_when_idle() {
    let parent = temp_dir();
    let scripts_dir = parent.join("scripts");
    fs::create_dir_all(&scripts_dir).expect("create scripts dir");
    fs::write(scripts_dir.join("run.sh"), "echo hi").expect("write script");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(scripts_dir.clone());
    registry.take_baseline();
    assert!(registry.check_for_changes().is_empty());
    assert!(
        registry.check_for_changes().is_empty(),
        "连续空转 NEVER 误报"
    );

    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn directory_snapshot_detects_directory_removal() {
    let parent = temp_dir();
    let skills_dir = parent.join("skills");
    fs::create_dir_all(&skills_dir).expect("create skills dir");

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(skills_dir.clone());
    registry.take_baseline();

    fs::remove_dir(&skills_dir).expect("remove skills dir");
    let changes = registry.check_for_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, FileChangeKind::Deleted);

    let _ = fs::remove_dir(&parent);
}
