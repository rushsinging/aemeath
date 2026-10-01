use super::*;
use std::fs;
use std::io::Write;

use std::sync::atomic::{AtomicU64, Ordering};

/// 测试唯一序号——每次调用自增，避免同进程内多个测试共享同一临时目录
/// 导致并行删除竞争（`File::create` 收到 EINVAL）。见 flaky 测试根因。
static TEMP_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let seq = TEMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aemeath_config_reload_test_{}_{}",
        std::process::id(),
        seq
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

#[test]
fn test_classify_change_key_non_prompt_asset() {
    let path = PathBuf::from("/home/user/.agents/aemeath.json");
    let key = classify_change_key(&path, &FileChangeKind::Modified);
    assert!(key.starts_with("other:modified:"));
}

#[test]
fn test_classify_change_key_instructions() {
    let path = PathBuf::from("/project/AGENTS.md");
    let key = classify_change_key(&path, &FileChangeKind::Deleted);
    assert!(key.starts_with("instructions:deleted:"));
}

#[test]
fn test_classify_change_key_guidance() {
    let path = PathBuf::from("/home/user/.agents/guidance/_default.md");
    let key = classify_change_key(&path, &FileChangeKind::Added);
    assert!(key.starts_with("guidance:added:"));
}

#[test]
fn test_check_config_changes_no_changes() {
    let dir = temp_dir();
    let file_path = dir.join("test.json");
    let mut f = fs::File::create(&file_path).unwrap();
    writeln!(f, "{{}}").unwrap();

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(file_path.clone());
    registry.take_baseline();

    let diff = check_config_changes(&mut registry);
    assert!(!diff.has_changes());
    assert!(diff.changed_keys.is_empty());

    let _ = fs::remove_file(&file_path);
    let _ = fs::remove_dir(&dir);
}

#[test]
fn test_check_config_changes_detects_modification() {
    let dir = temp_dir();
    let file_path = dir.join("config.json");
    let mut f = fs::File::create(&file_path).unwrap();
    writeln!(f, "{{\"key\": \"value1\"}}").unwrap();

    let mut registry = SourceSnapshotRegistry::new();
    registry.register(file_path.clone());
    registry.take_baseline();

    // 修改文件
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&file_path, "{\"key\": \"value2\"}").unwrap();

    let diff = check_config_changes(&mut registry);
    assert!(diff.has_changes());
    assert_eq!(diff.changes.len(), 1);
    assert_eq!(diff.changes[0].kind, FileChangeKind::Modified);
    assert_eq!(diff.changed_keys.len(), 1);

    let _ = fs::remove_file(&file_path);
    let _ = fs::remove_dir(&dir);
}
