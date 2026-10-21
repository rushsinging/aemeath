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
    // #2015 语义升级：aemeath.json 升为 config: 前缀（原 other:）。
    let path = PathBuf::from("/home/user/.agents/aemeath.json");
    let key = classify_change_key(&path, &FileChangeKind::Modified);
    assert!(key.starts_with("config:modified:"));
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

// ── #2015 watch 面扩展与 key 分类 ──

#[test]
fn test_classify_change_key_guardrails() {
    let path = PathBuf::from("/project/.agents/guardrails.md");
    assert_eq!(
        classify_change_key(&path, &FileChangeKind::Modified),
        "guardrails:modified:guardrails.md"
    );
}

#[test]
fn test_classify_change_key_tool_guidance() {
    let path = PathBuf::from("/home/user/.agents/tool_guidance.md");
    assert_eq!(
        classify_change_key(&path, &FileChangeKind::Added),
        "tool_guidance:added:tool_guidance.md"
    );
}

#[test]
fn test_classify_change_key_config_file() {
    let path = PathBuf::from("/home/user/.agents/aemeath.json");
    assert_eq!(
        classify_change_key(&path, &FileChangeKind::Modified),
        "config:modified:aemeath.json"
    );
}

#[test]
fn test_classify_change_key_skills_directory() {
    let path = PathBuf::from("/home/user/.agents/skills");
    assert_eq!(
        classify_change_key(&path, &FileChangeKind::Modified),
        "skills:modified:skills"
    );
}

#[test]
fn test_classify_change_key_scripts_directory() {
    let path = PathBuf::from("/project/.agents/scripts");
    assert_eq!(
        classify_change_key(&path, &FileChangeKind::Added),
        "scripts:added:scripts"
    );
}

#[test]
fn test_classify_change_key_other_still_fallbacks() {
    let path = PathBuf::from("/tmp/notes.txt");
    assert!(classify_change_key(&path, &FileChangeKind::Modified).starts_with("other:modified:"));
}

// ── #2015 防御复检（validate 段）──

#[test]
fn validate_component_changes_rejects_malformed_frontmatter() {
    let dir = temp_dir();
    let guardrails_path = dir.join("guardrails.md");
    fs::write(&guardrails_path, "---\n- just\n- a list\n---\n规则正文").expect("write");

    let rejections = validate_component_changes(&[FileChange {
        path: guardrails_path.clone(),
        kind: FileChangeKind::Modified,
    }]);
    assert_eq!(rejections.len(), 1, "frontmatter 非映射必须被拒");
    assert_eq!(rejections[0].path, guardrails_path);
    assert!(!rejections[0].reason.is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn validate_component_changes_accepts_well_formed_guardrails() {
    let dir = temp_dir();
    let guardrails_path = dir.join("guardrails.md");
    fs::write(&guardrails_path, "纯规则正文，无 frontmatter").expect("write");

    let rejections = validate_component_changes(&[FileChange {
        path: guardrails_path,
        kind: FileChangeKind::Modified,
    }]);
    assert!(rejections.is_empty(), "合法内容必须放行：{rejections:?}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn validate_component_changes_skips_deleted_files() {
    let dir = temp_dir();
    let tool_guidance_path = dir.join("tool_guidance.md");

    let rejections = validate_component_changes(&[FileChange {
        path: tool_guidance_path,
        kind: FileChangeKind::Deleted,
    }]);
    assert!(rejections.is_empty(), "删除是合法变更，无从校验内容");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn validate_component_changes_ignores_non_component_files() {
    let dir = temp_dir();
    let notes_path = dir.join("notes.md");
    fs::write(&notes_path, "---\n- 不是组件\n---\n").expect("write");

    let rejections = validate_component_changes(&[FileChange {
        path: notes_path,
        kind: FileChangeKind::Modified,
    }]);
    assert!(rejections.is_empty(), "非组件面文件不参与复检");

    let _ = fs::remove_dir_all(&dir);
}
