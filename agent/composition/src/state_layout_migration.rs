//! aemeath 运行时状态目录迁移（#2179）。
//!
//! 旧布局把运行时状态直接放在 `~/.agents/<leaf>/`（session / sessions /
//! harness / scoring / …），新布局统一收进 `~/.aemeath/state/<leaf>/`。
//!
//! 迁移契约（与 issue #2179 阶段 2 一致）：
//! - **幂等**：`state/.layout-migration.json` 存在即视为完成，重复执行不再复制；
//! - **不删数据**：全部按「复制」执行，旧目录原样保留（只读 fallback）；
//! - **先备份**：新状态目录已存在且非空时，先整体复制到
//!   `<aemeath_home>/state.bak-<epoch_ms>`，回滚即「删新目录 + 还原备份」；
//! - **不合并**：新旧目录同时存在时跳过该叶子并记 warning，绝不覆盖用户数据；
//! - **fail-open**：调用方按 warn 处理迁移失败，不阻断会话启动；失败时
//!   旧目录仍在，读取侧 fallback 仍可拿到数据。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use share::config::paths::RUNTIME_STATE_LEAVES;

/// 迁移结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StateLayoutMigrationReport {
    /// 本次从旧位置复制到新位置的叶子目录。
    pub migrated: Vec<String>,
    /// 已存在因而跳过的叶子目录。
    pub skipped: Vec<String>,
    /// 备份目录（仅当迁移前新状态目录非空时产生）。
    pub backup: Option<PathBuf>,
    /// 非致命问题（新旧目录并存、单叶子复制失败等）。
    pub warnings: Vec<String>,
    /// 命中完成标记，未做任何文件操作。
    pub already_migrated: bool,
}

impl StateLayoutMigrationReport {
    /// 是否存在需要用户关注的告警。
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// 执行运行时状态目录迁移。
///
/// `aemeath_home` 通常为 `share::config::paths::aemeath_home_dir()`；
/// `agents_dir` 通常为 `share::config::paths::global_agents_dir()`。
/// 显式传参使测试可用临时目录覆盖，不触碰真实用户数据。
pub fn migrate_state_layout(
    agents_dir: &Path,
    aemeath_home: &Path,
) -> Result<StateLayoutMigrationReport, String> {
    let state_dir = aemeath_home.join(share::config::paths::STATE_DIR_NAME);
    let marker = state_dir.join(crate::constants::STATE_LAYOUT_MARKER_FILE);
    if marker.exists() {
        return Ok(StateLayoutMigrationReport {
            already_migrated: true,
            ..StateLayoutMigrationReport::default()
        });
    }

    let mut report = StateLayoutMigrationReport::default();

    // 先备份已有新状态目录（幂等迁移中途失败后重试时保护已迁数据）。
    if dir_has_entries(&state_dir) {
        let backup = aemeath_home.join(format!("state.bak-{}", epoch_millis()));
        copy_dir_recursive(&state_dir, &backup).map_err(|error| {
            format!(
                "备份现状失败（{} -> {}）：{error}",
                state_dir.display(),
                backup.display()
            )
        })?;
        report.backup = Some(backup);
    }

    std::fs::create_dir_all(&state_dir)
        .map_err(|error| format!("创建状态根失败（{}）：{error}", state_dir.display()))?;

    for leaf in RUNTIME_STATE_LEAVES {
        // 迁移源必须来自调用方传入的 agents_dir（测试注入临时目录时不得
        // 触碰真实 ~/.agents 用户数据）。
        let source = agents_dir.join(leaf);
        let target = state_dir.join(leaf);
        if !source.is_dir() {
            continue;
        }
        if target.exists() {
            report.skipped.push((*leaf).to_string());
            report
                .warnings
                .push(format!("新路径已存在，跳过且不覆盖：{}", target.display()));
            continue;
        }
        match copy_dir_recursive(&source, &target) {
            Ok(()) => report.migrated.push((*leaf).to_string()),
            Err(error) => report.warnings.push(format!(
                "复制失败（{} -> {}）：{error}",
                source.display(),
                target.display()
            )),
        }
    }

    write_marker(&marker, agents_dir, &report)?;
    Ok(report)
}

/// 回滚一次迁移：删除新状态目录并从备份还原。
///
/// 返回错误时**不会**删除新目录——先校验备份可用，再执行替换。
pub fn rollback_state_layout(aemeath_home: &Path, backup: &Path) -> Result<(), String> {
    let state_dir = aemeath_home.join(share::config::paths::STATE_DIR_NAME);
    if !backup.is_dir() {
        return Err(format!("备份目录不存在：{}", backup.display()));
    }
    if state_dir.exists() {
        std::fs::remove_dir_all(&state_dir)
            .map_err(|error| format!("删除新状态目录失败（{}）：{error}", state_dir.display()))?;
    }
    copy_dir_recursive(backup, &state_dir)
        .map_err(|error| format!("还原备份失败（{}）：{error}", backup.display()))
}

/// 启动期入口：按生产路径执行一次迁移，失败只 warn 不阻断启动。
///
/// 幂等：命中 `state/.layout-migration.json` 时零文件操作直接返回。
pub fn ensure_state_layout() {
    let agents_dir = share::config::paths::global_agents_dir();
    let aemeath_home = share::config::paths::aemeath_home_dir();
    match migrate_state_layout(&agents_dir, &aemeath_home) {
        Ok(report) if report.already_migrated => {}
        Ok(report) => {
            if !report.migrated.is_empty() {
                log::info!(
                    target: crate::LOG_TARGET,
                    "[state-layout] 运行时状态迁移完成：migrated={:?} backup={:?}",
                    report.migrated,
                    report.backup
                );
            }
            for warning in &report.warnings {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[state-layout] {warning}"
                );
            }
        }
        Err(error) => log::warn!(
            target: crate::LOG_TARGET,
            "[state-layout] 迁移失败，本次继续使用旧 ~/.agents 布局：{error}"
        ),
    }
}

/// 装配点使用的状态根：显式注入的 `agents_dir`（非默认 `~/.agents`）视为状态根，
/// 用于测试与嵌入式调用保持目录隔离；默认路径走迁移后的
/// [`resolved_state_layout_root`]。
pub fn resolve_state_root_for(agents_dir: &Path) -> PathBuf {
    if agents_dir == share::config::paths::global_agents_dir() {
        resolved_state_layout_root()
    } else {
        agents_dir.to_path_buf()
    }
}

/// 运行时状态 dataset 根目录的解析结果。
///
/// 迁移完成后使用 `~/.aemeath/state`；未完成（标记缺失，通常意味着迁移
/// 失败）回退旧 `~/.agents`，保证只读历史与会话仍可继续，下一次启动重试迁移。
pub fn resolved_state_layout_root() -> PathBuf {
    let state_dir = share::config::paths::aemeath_state_dir();
    if state_dir
        .join(crate::constants::STATE_LAYOUT_MARKER_FILE)
        .exists()
    {
        state_dir
    } else {
        share::config::paths::global_agents_dir()
    }
}

fn write_marker(
    marker: &Path,
    agents_dir: &Path,
    report: &StateLayoutMigrationReport,
) -> Result<(), String> {
    let payload = format!(
        "{{\n  \"migrated_at_epoch_ms\": {},\n  \"agents_dir\": {},\n  \"migrated\": [{}],\n  \"skipped\": [{}],\n  \"backup\": {}\n}}\n",
        epoch_millis(),
        json_string(&agents_dir.display().to_string()),
        json_array(&report.migrated),
        json_array(&report.skipped),
        report
            .backup
            .as_ref()
            .map(|path| json_string(&path.display().to_string()))
            .unwrap_or_else(|| "null".to_string()),
    );
    std::fs::write(marker, payload)
        .map_err(|error| format!("写入迁移标记失败（{}）：{error}", marker.display()))
}

fn json_array(values: &[String]) -> String {
    values
        .iter()
        .map(|value| json_string(value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn json_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn dir_has_entries(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|entries| entries.filter_map(Result::ok).next().is_some())
        .unwrap_or(false)
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|error| format!("创建目录失败（{}）：{error}", target.display()))?;
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("读取目录失败（{}）：{error}", source.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("读取目录项失败（{}）：{error}", source.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取文件类型失败（{}）：{error}", entry.path().display()))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to).map_err(|error| {
                format!(
                    "复制文件失败（{} -> {}）：{error}",
                    from.display(),
                    to.display()
                )
            })?;
        } else if file_type.is_symlink() {
            copy_symlink(&from, &to)?;
        }
    }
    Ok(())
}

fn copy_symlink(source: &Path, target: &Path) -> Result<(), String> {
    let link_target = std::fs::read_link(source)
        .map_err(|error| format!("读取符号链接失败（{}）：{error}", source.display()))?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&link_target, target)
            .map_err(|error| format!("创建符号链接失败（{}）：{error}", target.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = link_target;
        Err(format!(
            "不支持在非 Unix 平台复制符号链接：{}",
            source.display()
        ))
    }
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(path, contents).expect("write file");
    }

    #[test]
    fn migrates_legacy_leaves_into_state_dir_and_keeps_old_copies() {
        let root = tempfile::tempdir().expect("temp root");
        let agents = root.path().join(".agents");
        let home = root.path().join(".aemeath");
        write_file(&agents.join("session/proj/a.dataset/manifest.json"), "{}");
        write_file(&agents.join("memory/proj/m.json"), "[]");

        let report = migrate_state_layout(&agents, &home).expect("migration");

        assert_eq!(report.migrated, vec!["session", "memory"]);
        assert!(home
            .join("state/session/proj/a.dataset/manifest.json")
            .exists());
        assert!(home.join("state/memory/proj/m.json").exists());
        // 旧目录只读保留：迁移不删除任何用户数据。
        assert!(agents.join("session/proj/a.dataset/manifest.json").exists());
        assert!(home
            .join("state")
            .join(crate::constants::STATE_LAYOUT_MARKER_FILE)
            .exists());
    }

    #[test]
    fn second_run_is_idempotent_via_marker() {
        let root = tempfile::tempdir().expect("temp root");
        let agents = root.path().join(".agents");
        let home = root.path().join(".aemeath");
        write_file(&agents.join("sessions/s1.lock"), "");

        let first = migrate_state_layout(&agents, &home).expect("first migration");
        assert_eq!(first.migrated, vec!["sessions"]);
        let second = migrate_state_layout(&agents, &home).expect("second migration");
        assert!(second.already_migrated);
        assert!(second.migrated.is_empty());
        assert!(second.backup.is_none());
    }

    #[test]
    fn existing_new_leaf_is_not_overwritten() {
        let root = tempfile::tempdir().expect("temp root");
        let agents = root.path().join(".agents");
        let home = root.path().join(".aemeath");
        write_file(&agents.join("memory/legacy.json"), "old");
        write_file(&home.join("state/memory/current.json"), "new");

        let report = migrate_state_layout(&agents, &home).expect("migration");

        assert_eq!(report.skipped, vec!["memory"]);
        assert!(report.has_warnings());
        assert_eq!(
            std::fs::read_to_string(home.join("state/memory/current.json")).expect("new file"),
            "new"
        );
        assert!(!home.join("state/memory/legacy.json").exists());
    }

    #[test]
    fn preexisting_state_dir_is_backed_up_before_migration() {
        let root = tempfile::tempdir().expect("temp root");
        let agents = root.path().join(".agents");
        let home = root.path().join(".aemeath");
        write_file(&home.join("state/meta/version.json"), "{\"v\":1}");
        write_file(&agents.join("audit/a.log"), "line");

        let report = migrate_state_layout(&agents, &home).expect("migration");

        let backup = report.backup.expect("backup path");
        assert!(backup.starts_with(home.clone()));
        assert!(backup.join("meta/version.json").exists());
        assert_eq!(report.migrated, vec!["audit"]);
    }

    #[test]
    fn rollback_restores_backup_and_drops_new_state() {
        let root = tempfile::tempdir().expect("temp root");
        let agents = root.path().join(".agents");
        let home = root.path().join(".aemeath");
        write_file(&home.join("state/meta/version.json"), "{\"v\":1}");
        write_file(&agents.join("audit/a.log"), "line");
        let report = migrate_state_layout(&agents, &home).expect("migration");
        let backup = report.backup.clone().expect("backup path");
        assert!(home.join("state/audit/a.log").exists());

        rollback_state_layout(&home, &backup).expect("rollback");

        assert!(home.join("state/meta/version.json").exists());
        assert!(!home.join("state/audit/a.log").exists());
        assert!(!home
            .join("state")
            .join(crate::constants::STATE_LAYOUT_MARKER_FILE)
            .exists());
    }

    #[test]
    fn rollback_rejects_missing_backup_without_touching_state() {
        let root = tempfile::tempdir().expect("temp root");
        let home = root.path().join(".aemeath");
        write_file(&home.join("state/meta/version.json"), "{\"v\":1}");

        let error =
            rollback_state_layout(&home, &root.path().join("missing")).expect_err("must fail");

        assert!(error.contains("备份目录不存在"));
        assert!(home.join("state/meta/version.json").exists());
    }

    #[test]
    fn empty_legacy_dirs_are_not_materialized() {
        let root = tempfile::tempdir().expect("temp root");
        let agents = root.path().join(".agents");
        let home = root.path().join(".aemeath");

        let report = migrate_state_layout(&agents, &home).expect("migration");

        assert!(report.migrated.is_empty());
        assert!(home.join("state").is_dir());
        assert!(home
            .join("state")
            .join(crate::constants::STATE_LAYOUT_MARKER_FILE)
            .exists());
    }
}
