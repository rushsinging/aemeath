//! 组件文件写入：递归解析 symlink，写最终普通文件。
//!
//! rename 会替换 symlink 本身（`AGENTS.md -> CLAUDE.md` 会变成普通 AGENTS.md），
//! 因此组件写入必须先解析 symlink 链，再直接写最终目标。原内容由
//! `ComponentVersionStore` 在写入前快照兜底。

use crate::constants::{ERROR_DOMAIN, MAX_COMPONENT_SYMLINK_DEPTH};
use crate::domain::errors::HarnessError;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 写入组件文本；若目标是 symlink，则递归解析到最终普通文件后写入。
pub(crate) fn write_component_text(
    path: &Path,
    text: &str,
    allowed_root: &Path,
) -> Result<(), HarnessError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("创建组件目录失败 {}: {error}", parent.display()),
            )
        })?;
    }
    std::fs::create_dir_all(allowed_root).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("创建组件父目录失败 {}: {error}", allowed_root.display()),
        )
    })?;
    let root = std::fs::canonicalize(allowed_root).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("解析组件根目录失败 {}: {error}", allowed_root.display()),
        )
    })?;

    let target = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => resolve_symlink_chain(path, &root)?,
        Ok(metadata) if metadata.is_file() => canonical_existing_file(path, &root)?,
        Ok(_) => {
            return Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("组件路径不是普通文件：{}", path.display()),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            canonical_new_file(path, &root)?
        }
        Err(error) => {
            return Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取组件文件元数据失败 {}: {error}", path.display()),
            ));
        }
    };

    write_direct(&target, text)?;
    log::debug!(
        target: crate::LOG_TARGET,
        "组件已直接写入最终文件：{}",
        target.display()
    );
    Ok(())
}

/// 递归解析 symlink 链，返回最终普通文件的 canonical 路径。
fn resolve_symlink_chain(path: &Path, root: &Path) -> Result<PathBuf, HarnessError> {
    let mut current = path.to_path_buf();
    let mut visited = HashSet::new();

    for _ in 0..=MAX_COMPONENT_SYMLINK_DEPTH {
        let normalized = normalize_parent(&current)?;
        if !visited.insert(normalized) {
            return Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("组件 symlink 循环：{}", path.display()),
            ));
        }

        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(HarnessError::storage(
                    ERROR_DOMAIN,
                    format!("组件 symlink 目标不存在：{}", current.display()),
                ));
            }
            Err(error) => {
                return Err(HarnessError::storage(
                    ERROR_DOMAIN,
                    format!("读取组件 symlink 目标失败 {}: {error}", current.display()),
                ));
            }
        };

        if metadata.file_type().is_symlink() {
            let link = std::fs::read_link(&current).map_err(|error| {
                HarnessError::storage(
                    ERROR_DOMAIN,
                    format!("读取组件 symlink 失败 {}: {error}", current.display()),
                )
            })?;
            current = if link.is_absolute() {
                link
            } else {
                current
                    .parent()
                    .ok_or_else(|| {
                        HarnessError::storage(
                            ERROR_DOMAIN,
                            format!("组件 symlink 缺少父目录：{}", current.display()),
                        )
                    })?
                    .join(link)
            };
            continue;
        }

        if !metadata.is_file() {
            return Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("组件 symlink 最终目标不是普通文件：{}", current.display()),
            ));
        }
        let canonical = std::fs::canonicalize(&current).map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("解析组件最终目标失败 {}: {error}", current.display()),
            )
        })?;
        ensure_within_root(&canonical, root)?;
        return Ok(canonical);
    }

    Err(HarnessError::storage(
        ERROR_DOMAIN,
        format!(
            "组件 symlink 超过最大解析深度 {MAX_COMPONENT_SYMLINK_DEPTH}：{}",
            path.display()
        ),
    ))
}

fn canonical_existing_file(path: &Path, root: &Path) -> Result<PathBuf, HarnessError> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("解析组件路径失败 {}: {error}", path.display()),
        )
    })?;
    ensure_within_root(&canonical, root)?;
    Ok(canonical)
}

fn canonical_new_file(path: &Path, root: &Path) -> Result<PathBuf, HarnessError> {
    let parent = path.parent().ok_or_else(|| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("组件路径缺少父目录：{}", path.display()),
        )
    })?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("解析组件父目录失败 {}: {error}", parent.display()),
        )
    })?;
    ensure_within_root(&canonical_parent, root)?;
    let file_name = path.file_name().ok_or_else(|| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("组件路径缺少文件名：{}", path.display()),
        )
    })?;
    Ok(canonical_parent.join(file_name))
}

/// 规范化父目录但保留最后一段，用于 symlink 循环检测。
fn normalize_parent(path: &Path) -> Result<PathBuf, HarnessError> {
    let parent = path.parent().ok_or_else(|| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("组件路径缺少父目录：{}", path.display()),
        )
    })?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("解析组件父目录失败 {}: {error}", parent.display()),
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("组件路径缺少文件名：{}", path.display()),
        )
    })?;
    Ok(canonical_parent.join(file_name))
}

fn ensure_within_root(path: &Path, root: &Path) -> Result<(), HarnessError> {
    if path.starts_with(root) {
        return Ok(());
    }
    Err(HarnessError::storage(
        ERROR_DOMAIN,
        format!(
            "组件 symlink 目标逃出允许根目录：{}（root={}）",
            path.display(),
            root.display()
        ),
    ))
}

fn write_direct(path: &Path, text: &str) -> Result<(), HarnessError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|error| {
            HarnessError::storage(
                ERROR_DOMAIN,
                format!("打开组件文件失败 {}: {error}", path.display()),
            )
        })?;
    file.write_all(text.as_bytes()).map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("写入组件文件失败 {}: {error}", path.display()),
        )
    })?;
    file.sync_all().map_err(|error| {
        HarnessError::storage(
            ERROR_DOMAIN,
            format!("刷新组件文件失败 {}: {error}", path.display()),
        )
    })
}

#[cfg(test)]
#[path = "file_write_tests.rs"]
mod file_write_tests;
