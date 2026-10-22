//! 文件变更快照注册表（IO 密集，放在 runtime 层）。
//!
//! 通过比较 (mtime, size) 判断文件是否被外部修改；mtime 倒退时
//! 用 sha256 兜底比对（覆盖编辑器原子替换 / NFS 场景）。
//!
//! 快照仅存内存，不落盘；重启进程即重建基线。

use sha2::{Digest, Sha256};
use share::config::file_snapshot::{FileChange, FileChangeKind, FileSnapshot};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 文件变更快照注册表。
///
/// 管理所有需要监控的文件路径，提供基线拍取和变更检测功能。
#[derive(Debug)]
pub struct SourceSnapshotRegistry {
    /// 已注册文件的路径列表。
    paths: Vec<PathBuf>,
    /// 已拍取的快照（path → snapshot）。
    snapshots: HashMap<PathBuf, FileSnapshot>,
}

impl SourceSnapshotRegistry {
    /// 创建空注册表。
    pub fn new() -> Self {
        Self {
            paths: Vec::new(),
            snapshots: HashMap::new(),
        }
    }

    /// 注册需要监控的文件路径。
    ///
    /// 重复注册同一路径会被忽略。
    pub fn register(&mut self, path: PathBuf) {
        if !self.paths.contains(&path) {
            self.paths.push(path);
        }
    }

    /// 批量注册文件路径。
    pub fn register_all(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            self.register(p);
        }
    }

    /// 拍取所有已注册文件的基线快照。
    ///
    /// 文件不存在时不拍快照（视为"空"），不报错。
    pub fn take_baseline(&mut self) {
        self.snapshots.clear();
        for path in &self.paths {
            if let Some(snap) = snapshot_entry(path) {
                self.snapshots.insert(path.clone(), snap);
            }
        }
    }

    /// 检测所有已注册文件的变更。
    ///
    /// 返回变更列表；无变更时返回空 Vec。
    /// 同时更新内部快照为最新状态。
    pub fn check_for_changes(&mut self) -> Vec<FileChange> {
        let mut changes = Vec::new();

        for path in &self.paths {
            let current_snap = snapshot_entry(path);
            let previous_snap = self.snapshots.get(path);

            match (previous_snap, current_snap) {
                // 条目之前存在，现在不存在 → 删除
                (Some(_), None) => {
                    changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Deleted,
                    });
                    self.snapshots.remove(path);
                }
                // 条目之前不存在，现在存在 → 新增
                (None, Some(snap)) => {
                    changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Added,
                    });
                    self.snapshots.insert(path.clone(), snap);
                }
                // 两份快照都在，比对
                (Some(prev), Some(curr)) => {
                    if entry_changed_with_path(prev, &curr, path) {
                        changes.push(FileChange {
                            path: path.clone(),
                            kind: FileChangeKind::Modified,
                        });
                        // 更新快照：存入内容指纹以便后续兜底比对
                        let updated = FileSnapshot {
                            mtime: curr.mtime,
                            size: curr.size,
                            sha256: compute_entry_sha256(path),
                        };
                        self.snapshots.insert(path.clone(), updated);
                    }
                }
                // 两份都没有 → 无变化
                (None, None) => {}
            }
        }

        changes
    }

    /// 获取已注册文件数量。
    pub fn len(&self) -> usize {
        self.paths.len()
    }
}

impl Default for SourceSnapshotRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 对单个监控条目拍快照（文件或目录）。不存在或不可读时返回 None。
///
/// 目录条目的快照：`mtime` 为目录自身 mtime，`size` 为直接子条目数，
/// `sha256` 为子条目清单指纹（子条目名 + 类型 + 子条目 mtime）。
/// 子文件内容修改不改目录元数据，因此目录变化判定 NEVER 走元数据快速路径，
/// 每次检测重算清单指纹（见 [`entry_changed_with_path`]）。
fn snapshot_entry(path: &Path) -> Option<FileSnapshot> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        let listing_sha = directory_listing_digest(path)?;
        let mut dir_mtime = meta.modified().ok()?;
        let entry_count = std::fs::read_dir(path).ok()?.count() as u64;
        // 目录 mtime 精度兜底：与清单指纹无关，仅保留元数据位。
        if dir_mtime == SystemTime::UNIX_EPOCH {
            dir_mtime = SystemTime::now();
        }
        return Some(FileSnapshot {
            mtime: dir_mtime,
            size: entry_count,
            sha256: Some(listing_sha),
        });
    }
    if !meta.is_file() {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let size = meta.len();
    // 初始快照即保存 sha256，避免首次 touch 被误判为内容变化。
    Some(FileSnapshot {
        mtime,
        size,
        sha256: compute_sha256(path),
    })
}

/// 计算目录直接子条目清单的 sha256（子条目名 + 类型 + 子条目 mtime）。
///
/// 清单参与排序保证稳定序；子条目 mtime 纳秒计入，使子文件内容
/// 修改（目录元数据不变）也能被检出。
fn directory_listing_digest(path: &Path) -> Option<[u8; 32]> {
    let mut listing: Vec<(String, bool, u128)> = Vec::new();
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        let entry_name = entry.file_name().to_string_lossy().to_string();
        let entry_meta = entry.metadata().ok()?;
        let is_dir = entry_meta.is_dir();
        let entry_mtime = entry_meta
            .modified()
            .ok()
            .map(|time| {
                time.duration_since(SystemTime::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        listing.push((entry_name, is_dir, entry_mtime));
    }
    listing.sort();
    let mut hasher = Sha256::new();
    for (entry_name, is_dir, entry_mtime) in listing {
        hasher.update(entry_name.as_bytes());
        hasher.update(if is_dir { b"/d" } else { b"/f" });
        hasher.update(entry_mtime.to_le_bytes());
    }
    let result = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&result);
    Some(digest)
}

/// 计算文件内容的 sha256。
fn compute_sha256(path: &Path) -> Option<[u8; 32]> {
    let data = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    Some(hash)
}

/// 按条目形态计算内容指纹（文件 = 内容 sha256；目录 = 清单指纹）。
fn compute_entry_sha256(path: &Path) -> Option<[u8; 32]> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => directory_listing_digest(path),
        _ => compute_sha256(path),
    }
}

/// 比对两份快照是否有变化（文件与目录策略不同）。
///
/// 文件策略：
/// 1. 先比较 (mtime, size)，一致则认为无变化
/// 2. 任一不同则用 sha256 兜底——处理编辑器原子替换（mtime 倒退但内容相同）
///
/// 目录策略：NEVER 走元数据快速路径——子文件内容修改不改目录 mtime 与
/// 条目数，必须每次重算清单指纹对比。
fn entry_changed_with_path(prev: &FileSnapshot, curr: &FileSnapshot, path: &Path) -> bool {
    if path.is_dir() {
        return match (prev.sha256, curr.sha256) {
            (Some(prev_sha), Some(curr_sha)) => prev_sha != curr_sha,
            // 任一侧缺指纹：保守认为变了（下次快照即补齐指纹）
            _ => true,
        };
    }
    file_changed_with_path(prev, curr, path)
}

fn file_changed_with_path(prev: &FileSnapshot, curr: &FileSnapshot, path: &Path) -> bool {
    // 快速路径：mtime 和 size 都没变
    if curr.mtime == prev.mtime && curr.size == prev.size {
        return false;
    }

    // mtime 或 size 有变化，需要 sha256 兜底
    let current_sha = match compute_sha256(path) {
        Some(h) => h,
        None => return true, // 无法读取，保守认为变了
    };

    match prev.sha256 {
        Some(prev_sha) => current_sha != prev_sha,
        None => {
            // 基线快照没存 sha256（首次检测到变化），保守认为变了
            // 下次更新快照后就有 sha256 了
            true
        }
    }
}

#[cfg(test)]
#[path = "snapshot_registry_tests.rs"]
mod tests;
