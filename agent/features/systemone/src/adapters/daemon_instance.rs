//! Scoring daemon 单实例锁与 sock 路径解析。
//!
//! 设计依据：`docs/design/02-modules/systemone/04-scoring-daemon.md` §4.2 / §5.1。

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::constants::{SCORING_DIR_NAME, SCORING_LOCK_FILE, SCORING_SOCK_FILE};

/// Daemon 落盘端点：scoring 目录、sock、lock。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonEndpoints {
    pub scoring_dir: PathBuf,
    pub sock: PathBuf,
    pub lock: PathBuf,
}

impl DaemonEndpoints {
    /// 默认路径：`{agents_dir}/scoring/scoring.sock` + `scoring.lock`。
    pub fn under_agents_dir(agents_dir: &Path) -> Self {
        Self::under_agents_dir_with_socket(agents_dir, None)
    }

    /// `socket_path` 覆盖 sock；lock **始终**落在 `{agents_dir}/scoring/scoring.lock`
    ///（单实例按 agents 根隔离，不随 sock 覆盖漂移）。
    pub fn under_agents_dir_with_socket(agents_dir: &Path, socket_path: Option<PathBuf>) -> Self {
        let scoring_dir = agents_dir.join(SCORING_DIR_NAME);
        let sock = socket_path.unwrap_or_else(|| scoring_dir.join(SCORING_SOCK_FILE));
        let lock = scoring_dir.join(SCORING_LOCK_FILE);
        Self {
            scoring_dir,
            sock,
            lock,
        }
    }
}

/// 单实例锁错误。
#[derive(Debug)]
pub enum DaemonLockError {
    AlreadyRunning,
    Io(io::Error),
}

impl std::fmt::Display for DaemonLockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => formatter.write_str("scoring daemon 已在运行（lock 被占用）"),
            Self::Io(error) => write!(formatter, "scoring daemon lock IO 失败：{error}"),
        }
    }
}

impl std::error::Error for DaemonLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::AlreadyRunning => None,
        }
    }
}

impl From<io::Error> for DaemonLockError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// 持有 `scoring.lock` 的排他文件锁；drop 时解锁。
#[derive(Debug)]
pub struct DaemonLockGuard {
    _file: File,
}

/// 获取非阻塞排他锁；已被占用 → [`DaemonLockError::AlreadyRunning`]。
pub fn acquire_daemon_lock(lock_path: &Path) -> Result<DaemonLockGuard, DaemonLockError> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)?;
    flock_exclusive_nonblock(&file)?;
    Ok(DaemonLockGuard { _file: file })
}

/// 在持有单实例锁的前提下，清除陈旧 sock 文件（仅当路径存在）。
///
/// 调用方 MUST 在 bind 监听前调用；无锁时本函数不可用（API 强制传入 guard）。
pub fn prepare_socket_path(sock_path: &Path, _lock: &DaemonLockGuard) -> io::Result<()> {
    if sock_path.exists() {
        fs::remove_file(sock_path)?;
    }
    if let Some(parent) = sock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn flock_exclusive_nonblock(file: &File) -> Result<(), DaemonLockError> {
    use rustix::fs::{flock, FlockOperation};
    match flock(file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(()),
        Err(error)
            if error == rustix::io::Errno::WOULDBLOCK || error == rustix::io::Errno::AGAIN =>
        {
            Err(DaemonLockError::AlreadyRunning)
        }
        Err(error) => Err(DaemonLockError::Io(io::Error::from(error))),
    }
}

#[cfg(not(unix))]
fn flock_exclusive_nonblock(_file: &File) -> Result<(), DaemonLockError> {
    Err(DaemonLockError::Io(io::Error::new(
        io::ErrorKind::Unsupported,
        "scoring daemon lock 仅支持 Unix",
    )))
}

#[cfg(test)]
#[path = "daemon_instance_tests.rs"]
mod tests;
