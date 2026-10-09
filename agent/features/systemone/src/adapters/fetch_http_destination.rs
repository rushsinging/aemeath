//! 目标路径安全：相对路径复核、父目录逐组件创建（拒符号链接）与
//! `create_new` + unix `O_NOFOLLOW` 打开（NEVER 覆盖、NEVER 跟随）。
//!
//! 父组件探测与随后的 `create_new` 打开之间存在 TOCTOU 竞态窗口
//! （与读路径校验同一边界声明）；`create_new` 是拒绝覆盖与末段符号链接的
//! 权威闸门，竞态内的任何父目录替换最终由安装端逐资产长度 + SHA-256 锁定。

use std::path::{Component, Path, PathBuf};

use crate::ports::{ArtifactFetchError, ArtifactFetchErrorKind};

/// 校验资产相对路径只含安全组件（防御性复核；manifest 校验已保证）。
pub(super) fn validate_relative_asset_path(relative_path: &str) -> Result<(), ArtifactFetchError> {
    let relative = Path::new(relative_path);
    if relative.is_absolute() {
        return Err(destination_rejected(format!(
            "资产路径不是相对路径：{relative_path}"
        )));
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(destination_rejected(format!(
                    "资产路径包含非法组件：{relative_path}"
                )));
            }
        }
    }
    Ok(())
}

/// 组装目标写入拒绝错误。
fn destination_rejected(detail: String) -> ArtifactFetchError {
    ArtifactFetchError {
        kind: ArtifactFetchErrorKind::DestinationRejected,
        detail,
    }
}

/// 准备目标文件路径：安全创建父目录（逐组件拒符号链接），拒绝已存在的目标（含末段链接）。
pub(super) fn prepare_destination(
    staging_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, ArtifactFetchError> {
    let destination = staging_root.join(relative_path);
    if let Some(relative_parent) = Path::new(relative_path).parent() {
        if !relative_parent.as_os_str().is_empty() {
            create_parent_directories(staging_root, relative_parent)?;
        }
    }
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => Err(destination_rejected(format!(
            "目标文件已存在，拒绝覆盖：{}",
            destination.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(destination),
        Err(error) => Err(destination_rejected(format!("目标文件不可探测：{error}"))),
    }
}

/// 逐组件创建父目录：缺失才创建（单组件 `create_dir`），既存形态必须是
/// 真实目录，符号链接 / 非目录一律拒绝，**NEVER** 穿过链接。
fn create_parent_directories(
    staging_root: &Path,
    relative_parent: &Path,
) -> Result<(), ArtifactFetchError> {
    let mut current = staging_root.to_path_buf();
    for component in relative_parent.components() {
        let Component::Normal(segment) = component else {
            return Err(destination_rejected(format!(
                "资产父目录包含非法组件：{}",
                relative_parent.display()
            )));
        };
        current.push(segment);
        match std::fs::symlink_metadata(&current) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    destination_rejected(format!("父目录创建失败：{}：{error}", current.display()))
                })?;
                let metadata = std::fs::symlink_metadata(&current).map_err(|error| {
                    destination_rejected(format!(
                        "父目录创建后不可探测：{}：{error}",
                        current.display()
                    ))
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(destination_rejected(format!(
                        "父目录形态异常（创建后复验）：{}",
                        current.display()
                    )));
                }
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(destination_rejected(format!(
                    "父目录是可疑符号链接：{}",
                    current.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(destination_rejected(format!(
                    "父目录不是目录：{}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) => {
                return Err(destination_rejected(format!(
                    "父目录不可探测：{}：{error}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

/// 打开目标文件：`create_new`（`O_CREAT | O_EXCL`，unix 另加 `O_NOFOLLOW`）
/// —— 已存在（含末段符号链接）直接失败，**NEVER 覆盖、NEVER 跟随**；
/// 创建 mode `0600`（owner 读写，不向同机其他用户暴露模型字节）；
/// 非 unix 至少 `create_new` fail-closed。
#[cfg(unix)]
pub(super) fn open_destination_file(
    destination: &Path,
) -> Result<tokio::fs::File, ArtifactFetchError> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NOFOLLOW 不由 std 暴露；值取自 rustix 的平台常量（与 libc 同一 ABI 定义）。
    let no_follow = rustix::fs::OFlags::NOFOLLOW.bits() as i32;
    let std_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(no_follow)
        .open(destination)
        .map_err(map_open_error)?;
    Ok(tokio::fs::File::from_std(std_file))
}

#[cfg(not(unix))]
pub(super) fn open_destination_file(
    destination: &Path,
) -> Result<tokio::fs::File, ArtifactFetchError> {
    let std_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(map_open_error)?;
    Ok(tokio::fs::File::from_std(std_file))
}

/// 打开失败分类：已存在（含符号链接）→ 拒绝覆盖；其余 → IO 拒绝。
fn map_open_error(error: std::io::Error) -> ArtifactFetchError {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => {
            destination_rejected(format!("目标文件已存在，拒绝覆盖：{error}"))
        }
        _ => destination_rejected(format!("目标文件创建失败：{error}")),
    }
}
