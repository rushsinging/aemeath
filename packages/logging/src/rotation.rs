//! 日志轮转与清理工具。
//! 无外部依赖，仅使用 std 和 chrono。

use chrono::{DateTime, Local};
use std::fs::{self};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{LOG_MAX_BACKUPS, LOG_MAX_BYTES, LOG_RETENTION_DAYS};

pub fn timestamp_rfc3339() -> String {
    let now: DateTime<Local> = Local::now();
    now.to_rfc3339()
}

pub(crate) fn prepare_log_path(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        cleanup_old_rotated_logs(parent)?;
    }
    rotate_if_needed(path, LOG_MAX_BYTES, LOG_MAX_BACKUPS)
}

pub(crate) fn rotate_if_needed(path: &Path, max_bytes: u64, max_backups: usize) -> io::Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if metadata.len() < max_bytes {
        return Ok(());
    }

    if max_backups == 0 {
        fs::remove_file(path)?;
        return Ok(());
    }

    for index in (1..=max_backups).rev() {
        let from = rotated_path(path, index);
        if !from.exists() {
            continue;
        }
        if index == max_backups {
            fs::remove_file(&from)?;
        } else {
            fs::rename(&from, rotated_path(path, index + 1))?;
        }
    }

    fs::rename(path, rotated_path(path, 1))
}

pub(crate) fn cleanup_old_rotated_logs(dir: &Path) -> io::Result<()> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_rotated_log_path(&path) {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if modified < cutoff {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!("{}.{}", file_name, index))
}

pub fn is_rotated_log_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some((base, suffix)) = file_name.rsplit_once('.') else {
        return false;
    };
    base.ends_with(".log") && suffix.chars().all(|c| c.is_ascii_digit())
}
