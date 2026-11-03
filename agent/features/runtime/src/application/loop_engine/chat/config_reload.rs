//! Turn 边界配置变更检测。
//!
//! 在每个 run 开始时，轮询配置/指令/guidance 文件是否有外部修改。
//! 检测到变更时返回 `ConfigDiff`，由调用方决定如何处理。

use super::snapshot_registry::SourceSnapshotRegistry;
use crate::application::constants::WATCH_DEPTH;
use share::config::file_snapshot::{FileChange, FileChangeKind};
use share::config::paths;
use std::path::{Path, PathBuf};

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

/// 收集需要监控的文件/目录路径。
///
/// 包括：
/// - 指令文件：从 cwd 向上 5 级祖先目录，每层 `CLAUDE.md` + `AGENTS.md`；
///   全局指令 `~/.agents/AGENTS.md`，fallback `~/.claude/CLAUDE.md`
/// - Guidance 文件：`~/.agents/guidance/` 下动态发现的 `*.md`
/// - 组件面（#2015 热载 watch 扩展）：guardrails（全局+项目）、
///   tool_guidance（全局）、skills 目录（全局+项目 `.agents`/`.claude`）；
///   scripts 目录已退役并入 skills（#2060）
///
/// config 文件（全局/项目 `aemeath.json`）**不在此列表**：配置热载由
/// `ConfigAppService::refresh_if_sources_changed()` 的内容指纹负责并携带
/// 生效边界（Run / SessionRestartRequired）；registry 再 watch 一次会产生
/// 第二条不带 scope 的 `ConfigReloaded`，导致回显重复且语义混淆。
pub fn collect_watched_files(cwd: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // ── 指令文件 ──
    // 项目指令：从 cwd 向上 5 级祖先目录，每层 CLAUDE.md + AGENTS.md
    for dir in paths::project_instruction_dirs(cwd, WATCH_DEPTH) {
        files.push(dir.join(paths::CLAUDE_MD));
        files.push(dir.join(paths::AGENTS_MD));
    }

    // 全局指令：~/.agents/AGENTS.md，fallback ~/.claude/CLAUDE.md
    files.push(expand_home(&paths::global_agents_md_path()));
    files.push(expand_home(&paths::old_global_claude_md_path()));

    // ── Guidance 文件（目录动态发现，替代静态文件清单）──
    let guidance_dir = expand_home(&paths::global_guidance_dir());
    files.extend(discover_markdown_files(&guidance_dir));

    // ── 组件面（#2015）──
    // guardrails：全局 + 项目
    files.push(expand_home(&paths::global_guardrails_path()));
    files.push(paths::project_guardrails_path(cwd));
    // tool_guidance 覆盖层：全局
    files.push(expand_home(&paths::global_tool_guidance_path()));
    // skills 目录（目录级监控）
    files.push(expand_home(&paths::global_skills_dir()));
    files.push(paths::project_skills_dir(cwd));
    files.push(paths::project_claude_skills_dir(cwd));
    files
}

/// 枚举目录下的 `*.md` 文件（目录不存在时返回空）。
fn discover_markdown_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut markdown_files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .collect();
    markdown_files.sort();
    markdown_files
}

/// 初始化配置变更快照注册表。
///
/// 注册所有监控文件并拍取基线快照。
pub fn init_snapshot_registry(cwd: &Path) -> SourceSnapshotRegistry {
    let files = collect_watched_files(cwd);
    let mut registry = SourceSnapshotRegistry::new();
    registry.register_all(files);
    registry.take_baseline();
    log::info!(target: crate::LOG_TARGET,
        "[config_reload] snapshot registry initialized with {} files",
        registry.len()
    );
    registry
}

/// 单个组件变更的防御复检拒绝记录（#2015 热载三段式的 validate 段产出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentReloadRejection {
    /// 校验失败的组件文件路径。
    pub path: PathBuf,
    /// 拒绝原因（呈现给日志与调用方通知）。
    pub reason: String,
}

/// 对组件类文件变更做防御性复检（frontmatter 形状）。
///
/// 覆盖 guardrails.md 与 tool_guidance.md（指令文件任意文本合法、
/// skills 目录变更在装配期逐文件校验，不在此列；scripts 目录已退役（#2060）。
/// 返回被拒列表；空列表 = 全部通过，可照常生效。
pub fn validate_component_changes(changes: &[FileChange]) -> Vec<ComponentReloadRejection> {
    changes
        .iter()
        .filter(|change| {
            let path_text = change.path.to_string_lossy();
            path_text.ends_with("guardrails.md") || path_text.ends_with("tool_guidance.md")
        })
        .filter_map(|change| {
            // 文件被删除时无从校验内容，视为通过（删除本身是合法变更）。
            if change.kind == FileChangeKind::Deleted {
                return None;
            }
            match harness::adapters::filesystem::validate_component_file_shape(&change.path) {
                Ok(()) => None,
                Err(error) => Some(ComponentReloadRejection {
                    path: change.path.clone(),
                    reason: error.message().to_string(),
                }),
            }
        })
        .collect()
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

    log::debug!(target: crate::LOG_TARGET,
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
    if path_str.contains("CLAUDE.md") || path_str.contains("AGENTS.md") {
        format!("instructions:{}:{}", prefix, file_name)
    } else if path_str.ends_with("guardrails.md") {
        format!("guardrails:{}:{}", prefix, file_name)
    } else if path_str.ends_with("tool_guidance.md") {
        // 注意 MUST 先于 guidance 分支：`tool_guidance.md` 也 contains("guidance")。
        format!("tool_guidance:{}:{}", prefix, file_name)
    } else if path_str.contains("guidance") {
        format!("guidance:{}:{}", prefix, file_name)
    } else if path_str.contains("/skills/") || path_str.ends_with("/skills") {
        format!("skills:{}:{}", prefix, file_name)
    } else {
        format!("other:{}:{}", prefix, file_name)
    }
}

/// 按来源分组统计组件类变更 key（如 `guidance:modified:x.md`）。
///
/// 返回顺序固定为 Guidance → Instructions → Skills（与 i18n 回显一致），
/// 只统计数量 > 0 的来源；`other:` / 被拒绝的 key 不参与分组。
pub fn component_reload_groups(
    changed_keys: &[String],
) -> Vec<(share::config::domain::reload::ComponentReloadSource, usize)> {
    use share::config::domain::reload::ComponentReloadSource;

    let mut groups = Vec::new();
    for source in [
        ComponentReloadSource::Guidance,
        ComponentReloadSource::Instructions,
        ComponentReloadSource::Skills,
    ] {
        let prefix = source.key_prefix();
        let count = changed_keys
            .iter()
            .filter(|key| key.starts_with(prefix))
            .count();
        if count > 0 {
            groups.push((source, count));
        }
    }
    groups
}

/// 组件热载的用户可见回显文本（按来源分组 + i18n，一行一个来源）。
///
/// `other:` / 被拒绝的 key 不参与；返回顺序固定 Guidance → Instructions → Skills。
pub fn component_reload_notices(language: &str, changed_keys: &[String]) -> Vec<String> {
    component_reload_groups(changed_keys)
        .into_iter()
        .map(|(source, file_count)| {
            share::i18n::runtime::config_reload::component_reload_notice(
                language, source, file_count,
            )
        })
        .collect()
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

#[cfg(test)]
#[path = "config_reload_tests.rs"]
mod tests;
