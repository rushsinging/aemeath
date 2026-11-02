//! adapter 层 I/O 与路径安全错误映射单点（#1865 D）。
//!
//! 原四份 `map_io`、三份 `reject_symlink`、两份 `map_sync`/`map_lock_io`、
//! 一份 `map_durability` 分散在 blob/dataset protocol 与文件系统 adapter 内，
//! 文案与严格度漂移。收敛于此：kind 判定（PermissionDenied → Io）与无跟随
//! 校验逻辑只维护一份，adapter 侧只保留业务上下文。

use crate::domain::{StorageError, StorageErrorKind};
use cap_std::fs::Dir;
use std::path::Path;

/// 通用 I/O 错误映射：权限拒绝单独成类，其余归 Io。
pub(super) fn map_io(error: std::io::Error) -> StorageError {
    let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageErrorKind::PermissionDenied
    } else {
        StorageErrorKind::Io
    };
    StorageError::new(kind, format!("存储 I/O 失败：{error}"))
}

/// 锁文件 I/O 错误映射。
pub(super) fn map_lock_io(error: std::io::Error) -> StorageError {
    StorageError::new(StorageErrorKind::Io, format!("存储键锁获取失败：{error}"))
}

/// fsync 类错误映射：平台无法兑现持久性要求。
pub(super) fn map_sync(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::UnsupportedDurability,
        format!("当前平台无法兑现持久性要求：{error}"),
    )
}

/// 持久性要求错误映射（blob/dataset 共用）。
pub(super) fn map_durability(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::UnsupportedDurability,
        format!("当前平台无法兑现持久性要求：{error}"),
    )
}

/// 无跟随符号链接校验：目标存在且为 symlink → InvalidKey；不存在 → 放行；
/// 其他 I/O 错误照常上抛（不吞错）。
pub(super) fn reject_symlink(dir: &Dir, path: &Path) -> Result<(), StorageError> {
    match dir.symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "目标路径是符号链接",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io(error)),
    }
}
