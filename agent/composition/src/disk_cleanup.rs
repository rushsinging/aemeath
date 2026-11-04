//! 落盘过期清理（`aemeath cleanup` 的过期清理面）。
//!
//! 与 [`crate::workflow_cleanup`]（无引用判据）互补：本模块按
//! `cleanup.retention_days` 的 age 判据清理 session 三件套、audit、
//! tmp 与 `~/.agents` 旧布局遗留。
//!
//! 设计约束：
//! - 核心扫描全部注入 roots / retention / now，零 env 依赖（可测）；
//! - 薄封装 [`run_disk_cleanup`] 只做 paths 解析与配置读取；
//! - 判定口径：目录条目取**目录树内最新 mtime**（AtomicDataset 写入
//!   走原子替换，父目录 mtime 会更新，树内最新值最保守），文件取
//!   自身 mtime；
//! - 幂等；dry-run 与执行同扫描口径；
//! - 单条目删除失败记入 errors 继续其余条目，不整体中止。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use share::config::cleanup::CleanupFace;

/// 单面清理报告。
#[derive(Debug, Clone, Default)]
pub struct FaceCleanupReport {
    /// 面名（展示用，如 `sessions` / `audit`）。
    pub face: &'static str,
    /// 该面被配置禁用（`0` 天）时为 true，扫描与删除均跳过。
    pub disabled: bool,
    /// 扫描条目数。
    pub scanned: usize,
    /// 命中删除的条目展示名（相对各 root 的路径或 id）。
    pub removed: Vec<String>,
    /// 命中条目的磁盘占用合计（dry-run 即预览回收量）。
    pub reclaimed_bytes: u64,
    /// 单条目失败明细（fail-soft，不中止整体）。
    pub errors: Vec<String>,
}

/// 全部过期清理面的汇总报告。
#[derive(Debug, Clone, Default)]
pub struct DiskCleanupReport {
    pub faces: Vec<FaceCleanupReport>,
    /// 只报告不删除的提示项（`~/.aemeath/state.bak-*` 一次性备份等）。
    pub advisories: Vec<AdvisoryItem>,
    pub dry_run: bool,
}

/// 报告型提示项（cleanup 永不自动删除）。
#[derive(Debug, Clone)]
pub struct AdvisoryItem {
    pub label: String,
    pub bytes: u64,
}

impl DiskCleanupReport {
    /// 是否存在任一面有待删除条目。
    pub fn has_pending(&self) -> bool {
        self.faces.iter().any(|face| !face.removed.is_empty())
    }

    /// 回收字节合计。
    pub fn total_reclaimed_bytes(&self) -> u64 {
        self.faces.iter().map(|face| face.reclaimed_bytes).sum()
    }
}

/// 计算路径磁盘占用：文件取长度，目录递归求和；失败按 0 计。
fn path_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            total += path_size(&entry.path());
        }
    }
    total
}

/// 目录树数据活跃度：**成员 mtime 优先**，空树回退目录自身 mtime。
///
/// 忽略根目录自身 mtime 的原因：目录 mtime 只反映成员增删，刚创建
/// 或刚搬移过的目录 mtime 恒新，不代表数据仍在写入；真实写入
/// （AtomicDataset 的 tmp+rename）总会同步刷新成员文件 mtime。
fn dir_newest_mtime(path: &Path, now: SystemTime) -> SystemTime {
    let mut newest: Option<SystemTime> = None;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            let child_newest = if child.is_dir() {
                dir_newest_mtime(&child, now)
            } else {
                entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(now)
            };
            newest = Some(match newest {
                Some(current) if current >= child_newest => current,
                _ => child_newest,
            });
        }
    }
    newest.unwrap_or_else(|| {
        fs::symlink_metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(now)
    })
}

/// 条目判定用 mtime：目录取树内最新，文件取自身。
fn entry_mtime(path: &Path, now: SystemTime) -> SystemTime {
    if path.is_dir() {
        dir_newest_mtime(path, now)
    } else {
        fs::symlink_metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(now)
    }
}

fn is_expired(path: &Path, days: u64, now: SystemTime) -> bool {
    match now.duration_since(entry_mtime(path, now)) {
        Ok(age) => age.as_secs() >= days * 86_400,
        Err(_) => false,
    }
}

/// 删除单个条目（目录递归 / 文件直接删），返回其占用字节数。
fn remove_entry(path: &Path) -> std::io::Result<u64> {
    let bytes = path_size(path);
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(bytes)
}

/// age 清理一个扁平/浅层目录下的**顶层条目**（文件或目录）。
///
/// 用于 tmp、audit、legacy 各遗留目录：顶层条目 mtime 过期即整条
/// 回收；目录不存在视为空面（scanned = 0）。
fn sweep_face(
    face: &'static str,
    roots: &[PathBuf],
    days: u64,
    now: SystemTime,
    dry_run: bool,
) -> FaceCleanupReport {
    let mut report = FaceCleanupReport {
        face,
        scanned: 0,
        ..FaceCleanupReport::default()
    };
    if days == 0 {
        report.disabled = true;
        return report;
    }
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) => {
                report
                    .errors
                    .push(format!("{}: 读取失败: {error}", root.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            report.scanned += 1;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if is_expired(&path, days, now) {
                if dry_run {
                    report.reclaimed_bytes += path_size(&path);
                    report.removed.push(rel_name(root, name));
                } else {
                    match remove_entry(&path) {
                        Ok(bytes) => {
                            report.reclaimed_bytes += bytes;
                            report.removed.push(rel_name(root, name));
                        }
                        Err(error) => {
                            report
                                .errors
                                .push(format!("{}: 删除失败: {error}", path.display()));
                        }
                    }
                }
            }
        }
    }
    report
}

fn rel_name(root: &Path, name: &str) -> String {
    // 展示名带 root 目录名前缀，区分 legacy 多目录同构条目。
    format!("{}/{name}", root.display())
}

/// session 三件套联动面。
///
/// 锚点是 dataset 目录（`<session_root>/<id>.dataset/`，会话本体在
/// 其内）：
/// - dataset 过期 → 联动回收 `<id>.dataset*`（含 stale-revision/bak
///   残留目录）、`<sessions_root>/<id>.lock`、
///   `<sessions_root>/<id>.background-process/`、
///   `<session_root>/accepted-input-ledger-<id>*`、
///   `<tool_results_root>/<id>/`；
/// - dataset 已不存在的孤儿（tool-results / lock / background-process /
///   ledger / stale-revision 残留）按自身 mtime 兜底过期回收。
#[allow(clippy::too_many_arguments)]
fn sweep_sessions_face(
    session_root: &Path,
    sessions_root: &Path,
    tool_results_root: &Path,
    days: u64,
    now: SystemTime,
    dry_run: bool,
) -> FaceCleanupReport {
    let mut report = FaceCleanupReport {
        face: "sessions",
        scanned: 0,
        ..FaceCleanupReport::default()
    };
    if days == 0 {
        report.disabled = true;
        return report;
    }
    if !session_root.is_dir() && !sessions_root.is_dir() && !tool_results_root.is_dir() {
        return report;
    }

    // 现存 session id（dataset 锚点，忽略 stale-revision/bak 后缀变体）。
    let mut live_ids: Vec<String> = Vec::new();
    if session_root.is_dir() {
        if let Ok(entries) = fs::read_dir(session_root) {
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if let Some(id) = name.strip_suffix(".dataset") {
                    live_ids.push(id.to_owned());
                }
            }
        }
    }

    // 面一：过期 dataset 联动三件套。
    for id in &live_ids {
        report.scanned += 1;
        let dataset = session_root.join(format!("{id}.dataset"));
        if !is_expired(&dataset, days, now) {
            continue;
        }
        // accepted-input-ledger 平铺残留按前缀匹配。
        let mut companions = vec![
            dataset,
            sessions_root.join(format!("{id}.lock")),
            sessions_root.join(format!("{id}.background-process")),
            tool_results_root.join(id),
        ];
        let ledger_prefix = format!("accepted-input-ledger-{id}");
        if let Ok(entries) = fs::read_dir(session_root) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(&ledger_prefix))
                {
                    companions.push(entry.path());
                }
            }
        }
        reclaim_companions(&companions, &format!("session {id}"), dry_run, &mut report);
    }

    // 面二：孤儿兜底（dataset 锚点已消失的残留）。
    sweep_orphans(
        sessions_root,
        &live_ids,
        |name| {
            name.strip_suffix(".lock")
                .or_else(|| name.strip_suffix(".background-process"))
                .map(str::to_owned)
        },
        days,
        now,
        dry_run,
        &mut report,
    );
    sweep_orphans(
        tool_results_root,
        &live_ids,
        |name| Some(name.to_owned()),
        days,
        now,
        dry_run,
        &mut report,
    );
    // dataset 目录内的变体残留（.dataset.stale-revision-* / .dataset.bak.*）
    // 与 accepted-input-ledger 孤儿：dataset 锚点不在时按自身 mtime 回收。
    sweep_orphans(
        session_root,
        &live_ids,
        |name| {
            if let Some(stripped) = name.strip_suffix(".dataset") {
                return Some(stripped.to_owned());
            }
            for marker in [".dataset.stale-revision-", ".dataset.bak."] {
                if let Some(index) = name.find(marker) {
                    return Some(name[..index].to_owned());
                }
            }
            if let Some(rest) = name.strip_prefix("accepted-input-ledger-") {
                // ledger 文件名形如 accepted-input-ledger-<id>.lock /
                // .quarantine.<hash>，取 <id>。
                let id = rest.split('.').next().unwrap_or(rest);
                return Some(id.to_owned());
            }
            None
        },
        days,
        now,
        dry_run,
        &mut report,
    );

    report
}

/// 联动回收一组伴随路径：聚合为**一个**报告条目（同一 session 的
/// dataset / lock / background-process / tool-results / ledger 属同次
/// 联动决策），字节数累加。
fn reclaim_companions(
    paths: &[PathBuf],
    label: &str,
    dry_run: bool,
    report: &mut FaceCleanupReport,
) {
    let mut reclaimed = 0u64;
    let mut hit = 0usize;
    let mut failed = false;
    for path in paths {
        if !path.exists() {
            continue;
        }
        hit += 1;
        if dry_run {
            reclaimed += path_size(path);
        } else {
            match remove_entry(path) {
                Ok(bytes) => reclaimed += bytes,
                Err(error) => {
                    report
                        .errors
                        .push(format!("{}: 删除失败: {error}", path.display()));
                    failed = true;
                }
            }
        }
    }
    if hit > 0 && !failed {
        report.reclaimed_bytes += reclaimed;
        report.removed.push(label.to_owned());
    } else if hit > 0 {
        report.reclaimed_bytes += reclaimed;
        report.removed.push(format!("{label}（部分失败）"));
    }
}

/// 孤儿兜底：`root` 下条目经 `extract_id` 归属 session id；
/// 归属 id 不在 `live_ids`（dataset 锚点已消失）且自身过期时回收。
fn sweep_orphans(
    root: &Path,
    live_ids: &[String],
    extract_id: impl Fn(&str) -> Option<String>,
    days: u64,
    now: SystemTime,
    dry_run: bool,
    report: &mut FaceCleanupReport,
) {
    if !root.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(id) = extract_id(&name) else {
            continue;
        };
        report.scanned += 1;
        if live_ids.iter().any(|live| live == &id) {
            continue;
        }
        let path = entry.path();
        if is_expired(&path, days, now) {
            if dry_run {
                report.reclaimed_bytes += path_size(&path);
                report.removed.push(format!("orphan {name}"));
            } else {
                match remove_entry(&path) {
                    Ok(bytes) => {
                        report.reclaimed_bytes += bytes;
                        report.removed.push(format!("orphan {name}"));
                    }
                    Err(error) => {
                        report
                            .errors
                            .push(format!("{}: 删除失败: {error}", path.display()));
                    }
                }
            }
        }
    }
}

/// 过期清理全部面（roots / retention / now 全注入）。
pub fn run_disk_cleanup_with_roots(
    roots: &DiskCleanupRoots,
    retention: &share::config::cleanup::CleanupRetentionDaysConfig,
    now: SystemTime,
    dry_run: bool,
) -> DiskCleanupReport {
    let mut faces = Vec::new();

    let sessions_days = retention.effective_days(CleanupFace::Sessions);
    faces.push(match sessions_days {
        Some(days) => sweep_sessions_face(
            &roots.session_datasets,
            &roots.sessions,
            &roots.tool_results,
            days,
            now,
            dry_run,
        ),
        None => disabled_face("sessions"),
    });

    let audit_days = retention.effective_days(CleanupFace::Audit);
    faces.push(match audit_days {
        Some(days) => sweep_face(
            "audit",
            std::slice::from_ref(&roots.audit_usage),
            days,
            now,
            dry_run,
        ),
        None => disabled_face("audit"),
    });

    let tmp_days = retention.effective_days(CleanupFace::Tmp);
    faces.push(match tmp_days {
        Some(days) => sweep_face("tmp", std::slice::from_ref(&roots.tmp), days, now, dry_run),
        None => disabled_face("tmp"),
    });

    let legacy_days = retention.effective_days(CleanupFace::Legacy);
    faces.push(match legacy_days {
        Some(days) => sweep_face("legacy", &roots.legacy_dirs, days, now, dry_run),
        None => disabled_face("legacy"),
    });

    DiskCleanupReport {
        faces,
        advisories: collect_state_backup_advisories(&roots.aemeath_home),
        dry_run,
    }
}

/// 收集 `~/.aemeath/state.bak-*` 一次性备份目录：只报告体积，永不删除。
fn collect_state_backup_advisories(aemeath_home: &Path) -> Vec<AdvisoryItem> {
    let Ok(entries) = fs::read_dir(aemeath_home) else {
        return Vec::new();
    };
    let mut advisories = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with("state.bak") && entry.path().is_dir() {
            advisories.push(AdvisoryItem {
                label: name,
                bytes: path_size(&entry.path()),
            });
        }
    }
    advisories.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    advisories
}

fn disabled_face(face: &'static str) -> FaceCleanupReport {
    FaceCleanupReport {
        face,
        disabled: true,
        ..FaceCleanupReport::default()
    }
}

/// 过期清理面的根目录集合（由 [`run_disk_cleanup`] 从 paths 解析）。
#[derive(Debug, Clone)]
pub struct DiskCleanupRoots {
    /// `state/session/`：dataset 与 accepted-input-ledger 落盘根。
    pub session_datasets: PathBuf,
    /// `state/sessions/`：会话 lock 与 background-process 残留。
    pub sessions: PathBuf,
    /// `state/tool-results/`：per-session 工具结果目录。
    pub tool_results: PathBuf,
    /// `state/audit/usage/`：审计 jsonl。
    pub audit_usage: PathBuf,
    /// `state/tmp/`。
    pub tmp: PathBuf,
    /// `~/.agents` 旧布局遗留 state 目录（workflows 真相源、memory、
    /// skills/hooks/config 等组件面不在此列）。
    pub legacy_dirs: Vec<PathBuf>,
    /// `~/.aemeath`（advisory 扫描用，不参与删除）。
    pub aemeath_home: PathBuf,
}

impl DiskCleanupRoots {
    /// 从全局 paths 解析真实根目录。
    pub fn resolve() -> Self {
        use share::config::paths::{
            aemeath_home_dir, global_agents_dir, global_audit_dir, global_session_datasets_dir,
            global_sessions_dir, global_tmp_dir, global_tool_results_dir,
        };
        let agents_root = global_agents_dir();
        let legacy_leaves = [
            "sessions",
            "session",
            "tool-results",
            "tool-result",
            "tool_outputs",
            "audit",
            "tmp",
        ];
        Self {
            session_datasets: global_session_datasets_dir(),
            sessions: global_sessions_dir(),
            tool_results: global_tool_results_dir(),
            audit_usage: global_audit_dir().join("usage"),
            tmp: global_tmp_dir(),
            legacy_dirs: legacy_leaves
                .into_iter()
                .map(|leaf| agents_root.join(leaf))
                .collect(),
            aemeath_home: aemeath_home_dir(),
        }
    }
}

/// 生产入口：解析 paths + 读配置快照后执行。
///
/// `retention` 传 `None` 时使用默认保留天数（30 天）。
pub fn run_disk_cleanup(
    retention: Option<share::config::cleanup::CleanupRetentionDaysConfig>,
    dry_run: bool,
) -> Result<DiskCleanupReport, String> {
    let retention = retention.unwrap_or_default();
    let report = run_disk_cleanup_with_roots(
        &DiskCleanupRoots::resolve(),
        &retention,
        SystemTime::now(),
        dry_run,
    );
    Ok(report)
}

#[cfg(test)]
#[path = "disk_cleanup_tests.rs"]
mod tests;
