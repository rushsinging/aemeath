//! 文件变更快照的纯类型定义。
//!
//! 这些类型用于在不同模块间传递文件变更信息，不包含任何 IO 逻辑。
//! 实际的快照检测逻辑（SourceSnapshotRegistry）位于 runtime 的 config_reload 模块。

use std::path::PathBuf;
use std::time::SystemTime;

/// 单个文件的快照信息。
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    /// 文件最后修改时间。
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
