//! Turn 边界配置变更检测。
//!
//! 在每个 turn 开始时，轮询配置/指令/guidance 文件是否有外部修改。
//! 检测到变更时返回 `ConfigDiff`，由调用方决定如何处理。

use super::snapshot_registry::SourceSnapshotRegistry;
use share::config::paths;
use share::config::snapshot::{FileChange, FileChangeKind};
use share::config::GuidanceReloadPolicy;
use std::path::{Path, PathBuf};
use crate::LOG_TARGET;

/// 配置变更差异。
#[derive(Debug, Clone)]
pub struct ConfigDiff {
    /// 变更的文件列表。
    pub changes: Vec<FileChange>,
    /// 变更的配置 key 列表（用于 SDK 事件通知）。
    pub changed_keys: Vec<String>,
}

impl ConfigDiff {
    /// 是否有任何变更。
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }
}

/// 收集需要监控的文件路径。
///
/// 包括：
/// - 配置文件：`~/.agents/aemeath.json`、`{cwd}/.agents/aemeath.json`、`{cwd}/.claude/settings.json`
/// - 指令文件：从 cwd 向上 5 级祖先目录，每层 `CLAUDE.md` + `AGENTS.md`；
///   全局指令 `~/.agents/AGENTS.md`，fallback `~/.claude/CLAUDE.md`
/// - Guidance 文件：`~/.agents/guidance/_default.md`、`~/.agents/guidance/_reasoning.md`
pub fn collect_watched_files(cwd: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // ── 配置文件 ──
    // 全局配置
    let global_config = expand_home(&paths::global_config_path());
    files.push(global_config);

    // 项目级配置
    let project_config = cwd
        .join(paths::AGENTS_DIR_NAME)
        .join(paths::NEW_CONFIG_FILE);
    files.push(project_config);

    // Claude Code 兼容配置
    let claude_settings = cwd.join(paths::CLAUDE_DIR_NAME).join(paths::SETTINGS_FILE);
    files.push(claude_settings);

    // ── 指令文件 ──
    // 项目指令：从 cwd 向上 5 级祖先目录，每层 CLAUDE.md + AGENTS.md
    const WATCH_DEPTH: u32 = 5;
    for dir in paths::project_instruction_dirs(cwd, WATCH_DEPTH) {
        files.push(dir.join(paths::CLAUDE_MD));
        files.push(dir.join(paths::AGENTS_MD));
    }

    // 全局指令：~/.agents/AGENTS.md，fallback ~/.claude/CLAUDE.md
    files.push(expand_home(&paths::global_agents_md_path()));
    files.push(expand_home(&paths::old_global_claude_md_path()));

    // ── Guidance 文件（静态已知的） ──
    let guidance_dir = expand_home(&paths::global_guidance_dir());
    files.push(guidance_dir.join("_default.md"));
    files.push(guidance_dir.join("_reasoning.md"));

    files
}

/// 初始化配置变更快照注册表。
///
/// 注册所有监控文件并拍取基线快照。
pub fn init_snapshot_registry(cwd: &Path) -> SourceSnapshotRegistry {
    let files = collect_watched_files(cwd);
    let mut registry = SourceSnapshotRegistry::new();
    registry.register_all(files);
    registry.take_baseline();
    log::info!(target: LOG_TARGET,
        "[config_reload] snapshot registry initialized with {} files",
        registry.len()
    );
    registry
}

/// 检测配置变更。
///
/// 对比当前文件状态与基线快照，返回 `ConfigDiff`。
/// 无变更时返回空 diff。
pub fn check_config_changes(registry: &mut SourceSnapshotRegistry) -> ConfigDiff {
    let changes = registry.check_for_changes();

    if changes.is_empty() {
        return ConfigDiff {
            changes: Vec::new(),
            changed_keys: Vec::new(),
        };
    }

    log::info!(target: LOG_TARGET,
        "[config_reload] detected {} file change(s)",
        changes.len()
    );

    let changed_keys = changes
        .iter()
        .map(|c| classify_change_key(&c.path, &c.kind))
        .collect();

    ConfigDiff {
        changes,
        changed_keys,
    }
}

/// 将文件变更映射为配置 key 名称（用于 SDK 事件通知）。
fn classify_change_key(path: &Path, kind: &FileChangeKind) -> String {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let prefix = match kind {
        FileChangeKind::Modified => "modified",
        FileChangeKind::Deleted => "deleted",
        FileChangeKind::Added => "added",
    };

    // 根据文件路径判断配置类型
    let path_str = path.to_string_lossy();
    if path_str.contains("aemeath.json") || path_str.contains("settings.json") {
        format!("config:{}:{}", prefix, file_name)
    } else if path_str.contains("CLAUDE.md") || path_str.contains("AGENTS.md") {
        format!("instructions:{}:{}", prefix, file_name)
    } else if path_str.contains("guidance") {
        format!("guidance:{}:{}", prefix, file_name)
    } else {
        format!("other:{}:{}", prefix, file_name)
    }
}

/// 将 `~` 展开为 home 目录。
fn expand_home(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

/// 解析当前 guidance reload_policy 配置。
///
/// 读取全局配置文件中的 `guidance.reload_policy` 字段；
/// 若读取失败或字段缺失，返回默认值 `Remind`。
pub fn resolve_guidance_reload_policy() -> GuidanceReloadPolicy {
    // 尝试读取全局配置
    let config_path = expand_home(&paths::global_config_path());
    if let Ok(data) = std::fs::read_to_string(&config_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) {
            if let Some(policy_str) = value
                .get("guidance")
                .and_then(|g| g.get("reload_policy"))
                .and_then(|p| p.as_str())
            {
                return match policy_str {
                    "inject" => GuidanceReloadPolicy::Inject,
                    "remind" => GuidanceReloadPolicy::Remind,
                    "confirm" => GuidanceReloadPolicy::Confirm,
                    _ => {
                        log::warn!(target: LOG_TARGET,
                            "[config_reload] unknown guidance.reload_policy '{}', using default",
                            policy_str
                        );
                        GuidanceReloadPolicy::Remind
                    }
                };
            }
        }
    }
    GuidanceReloadPolicy::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aemeath_config_reload_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_classify_change_key_config() {
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
}
