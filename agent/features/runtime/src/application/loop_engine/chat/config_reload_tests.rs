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
    // aemeath.json 已从 watch 列表移除（config 由 ConfigAppService 指纹负责），
    // 因此不再有 config: 专属前缀，走 other: 兜底。
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
fn test_classify_change_key_config_file_falls_back_to_other() {
    let path = PathBuf::from("/home/user/.agents/aemeath.json");
    assert_eq!(
        classify_change_key(&path, &FileChangeKind::Modified),
        "other:modified:aemeath.json"
    );
}

#[test]
fn watched_files_exclude_config_json() {
    // config 文件变化由 ConfigAppService::refresh_if_sources_changed 的 SHA 指纹
    // 检测并 emit；registry 再 watch 一次会重复通知且不带 scope 语义。
    let cwd = temp_dir();
    let files = collect_watched_files(&cwd);
    let config_json: Vec<&PathBuf> = files
        .iter()
        .filter(|path| path.ends_with("aemeath.json"))
        .collect();
    assert!(
        config_json.is_empty(),
        "watch 列表 MUST NOT 包含 config 文件，实际: {config_json:?}"
    );
    let _ = fs::remove_dir_all(&cwd);
}

#[test]
fn component_reload_notices_render_one_i18n_line_per_source() {
    let keys = vec![
        "guidance:modified:_default.md".to_string(),
        "guidance:added:extra.md".to_string(),
        "skills:added:skills".to_string(),
        "other:modified:notes.txt".to_string(),
    ];
    assert_eq!(
        component_reload_notices("zh", &keys),
        vec![
            "[guidance] 已重载（2 个文件）。".to_string(),
            "[skills] 已重载（1 个文件）。".to_string(),
        ]
    );
    assert_eq!(
        component_reload_notices("en", &keys),
        vec![
            "[guidance] reloaded (2 file(s)).".to_string(),
            "[skills] reloaded (1 file(s)).".to_string(),
        ]
    );
}

#[test]
fn component_reload_groups_counts_keys_by_source_in_stable_order() {
    let keys = vec![
        "guidance:modified:_default.md".to_string(),
        "guidance:added:extra.md".to_string(),
        "skills:added:skills".to_string(),
        "instructions:deleted:AGENTS.md".to_string(),
        "other:modified:notes.txt".to_string(),
    ];
    let groups = component_reload_groups(&keys);
    assert_eq!(
        groups,
        vec![
            (
                share::config::domain::reload::ComponentReloadSource::Guidance,
                2
            ),
            (
                share::config::domain::reload::ComponentReloadSource::Instructions,
                1
            ),
            (
                share::config::domain::reload::ComponentReloadSource::Skills,
                1
            ),
        ],
        "分组 MUST 按 Guidance→Instructions→Skills 固定顺序，且忽略 other: key"
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
fn test_classify_change_key_scripts_directory_falls_back() {
    // #2060：scripts 目录退役并入 skills——不再专属分类，走 other fallback
    let path = PathBuf::from("/project/.agents/scripts");
    assert!(classify_change_key(&path, &FileChangeKind::Added).starts_with("other:added:"));
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
