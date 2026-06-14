//! 文件变更快照检测。
//!
//! 通过比较 (mtime, size) 判断文件是否被外部修改；mtime 倒退时
//! 用 sha256 兜底比对（覆盖编辑器原子替换 / NFS 场景）。
//!
//! 快照仅存内存，不落盘；重启进程即重建基线。

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 单个文件的快照信息。
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    /// 文件最后修改时间（秒精度）。
    pub mtime: SystemTime,
    /// 文件大小（字节）。
    pub size: u64,
    /// 内容 sha256（仅在需要兜底比对时计算，平时为 None）。
    pub sha256: Option<[u8; 32]>,
}

/// 单个文件的变更检测结果。
#[derive(Debug, Clone)]
pub struct FileChange {
    /// 变更文件的路径。
    pub path: PathBuf,
    /// 变更类型。
    pub kind: FileChangeKind,
}

/// 文件变更类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChangeKind {
    /// 文件被修改（mtime/size 变化或 sha256 不同）。
    Modified,
    /// 文件被删除。
    Deleted,
    /// 文件新增（首次检测到）。
    Added,
}

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
            if let Some(snap) = snapshot_file(path) {
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
            let current_snap = snapshot_file(path);
            let previous_snap = self.snapshots.get(path);

            match (previous_snap, current_snap) {
                // 文件之前存在，现在不存在 → 删除
                (Some(_), None) => {
                    changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Deleted,
                    });
                    self.snapshots.remove(path);
                }
                // 文件之前不存在，现在存在 → 新增
                (None, Some(snap)) => {
                    changes.push(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Added,
                    });
                    self.snapshots.insert(path.clone(), snap);
                }
                // 两份快照都在，比对
                (Some(prev), Some(curr)) => {
                    if file_changed_with_path(prev, &curr, path) {
                        changes.push(FileChange {
                            path: path.clone(),
                            kind: FileChangeKind::Modified,
                        });
                        // 更新快照：存入 sha256 以便后续兜底比对
                        let updated = FileSnapshot {
                            mtime: curr.mtime,
                            size: curr.size,
                            sha256: compute_sha256(path),
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

    /// 是否已注册任何文件。
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

impl Default for SourceSnapshotRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 对单个文件拍快照。文件不存在或无法读取时返回 None。
fn snapshot_file(path: &Path) -> Option<FileSnapshot> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let size = meta.len();
    // 初始快照不计算 sha256（节省 IO），仅在需要兜底比对时计算
    Some(FileSnapshot {
        mtime,
        size,
        sha256: None,
    })
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

/// 比对两份快照是否有变化（可访问文件路径做 sha256 兜底）。
///
/// 策略：
/// 1. 先比较 (mtime, size)，一致则认为无变化
/// 2. 任一不同则用 sha256 兜底——处理编辑器原子替换（mtime 倒退但内容相同）
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

/// 精确比对：计算当前文件 sha256 与快照比对。
///
/// 用于 mtime 变化但需要确认内容是否真的变了的场景。
pub fn content_has_changed(path: &Path, snapshot: &FileSnapshot) -> bool {
    let current_sha = match compute_sha256(path) {
        Some(h) => h,
        None => return true, // 无法读取，视为变更
    };

    match snapshot.sha256 {
        Some(prev_sha) => current_sha != prev_sha,
        None => {
            // 之前没存 sha256，说明是基线快照（没算过 sha256）
            // 此时 mtime/size 已经不同，内容大概率变了
            // 但我们保守一点：算一次 sha256，如果 mtime 不同但内容相同则不认为是变更
            // 这需要读旧文件，但旧快照没存 sha256，所以我们直接返回 true
            // （第一次检测到变化时就认为变了，然后更新快照包含 sha256）
            true
        }
    }
}

#[cfg(test)]
mod tests {
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

        // 未修改文件，不应检测到变更
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

        // 修改文件内容
        // 需要等一下让 mtime 变化（某些文件系统精度是秒级）
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

        // 删除文件
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
        // 基线时文件不存在，无快照

        // 创建文件
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

        // 第一次检测：修改
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&path, "world").unwrap();
        let changes = registry.check_for_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, FileChangeKind::Modified);

        // 第二次检测：无变化（快照已更新）
        let changes = registry.check_for_changes();
        assert!(changes.is_empty(), "expected no changes after update, got {:?}", changes);

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

        // 空文件未修改，不应检测到变更
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

        // 不存在的文件不应拍快照
        assert!(registry.snapshots.is_empty());

        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn test_content_has_changed_detects_real_change() {
        let dir = temp_dir();
        let path = write_file(&dir, "test_content.txt", "hello");

        let snap = FileSnapshot {
            mtime: SystemTime::now(),
            size: 5,
            sha256: compute_sha256(&path),
        };

        // 内容未变，不应有变化
        assert!(!content_has_changed(&path, &snap));

        // 修改内容
        fs::write(&path, "world").unwrap();
        assert!(content_has_changed(&path, &snap));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
