//! Session 级 lock 文件（#636 D3）。
//!
//! 防止两个 aemeath 实例同时操作同一 session_id 造成数据互相覆盖。
//!
//! Lock 文件路径：`<sessions_dir>/{session_id}.lock`，内容为 JSON：
//! `{ "pid": <u32>, "created_at": "<rfc3339>", "hostname": "<string>" }`。
//!
//! acquire 语义：
//! - 文件不存在 → 直接创建。
//! - 文件存在但 pid 已死 → 视为过期，允许接管。
//! - 文件存在且 pid 仍活 → 返回 `LockError::HeldAlive`，由上层决定是否强制接管。
//!
//! release 语义：
//! - 显式调用 `release()` 删除文件。
//! - 进程被 kill -9 时 Drop 不执行；下次启动由 pid liveness 检测兜底。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use share::config::paths::global_sessions_dir;

/// Lock 文件的元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLockMeta {
    /// 持有 lock 的进程 pid。
    pub pid: u32,
    /// Lock 创建时间（RFC3339）。
    pub created_at: String,
    /// 主机名，便于跨机器调试。
    pub hostname: String,
}

/// acquire 失败原因。
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Lock 被另一个活跃进程持有。上层决定是否调用 `force_acquire` 接管。
    #[error("session lock held by live pid {pid} (acquired {created_at} on {hostname})")]
    HeldAlive {
        pid: u32,
        created_at: String,
        hostname: String,
        path: PathBuf,
    },
    /// 文件存在但内容损坏。
    #[error("session lock file corrupt at {path}: {reason}")]
    Corrupt { path: PathBuf, reason: String },
    /// 底层 IO 错误。
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// 已持有的 session lock guard。Drop 时尝试删除 lock 文件。
#[derive(Debug)]
pub struct SessionLock {
    path: PathBuf,
    released: bool,
}

impl SessionLock {
    /// 路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_path(session_id: &str) -> PathBuf {
    global_sessions_dir().join(format!("{session_id}.lock"))
}

fn pid_alive(pid: u32) -> bool {
    // Unix：kill(pid, 0) 返回 0 表示进程存在，ESRCH 表示不存在。
    // 注意：信号 0 不实际发送信号，仅做存在性检查。
    unsafe {
        let rc = libc::kill(pid as i32, 0);
        if rc == 0 {
            true
        } else {
            // 进程不存在返回 ESRCH；其他错误（EPERM/无权限）说明进程存在但无权发信号。
            let err = std::io::Error::last_os_error();
            err.raw_os_error() != Some(libc::ESRCH)
        }
    }
}

/// 读取 lock 文件元数据（若存在）。
fn read_meta(path: &Path) -> io::Result<Option<SessionLockMeta>> {
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<SessionLockMeta>(&s) {
            Ok(m) => Ok(Some(m)),
            Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// 写入 lock 文件（原子写：先写 tmp，再 rename）。
fn write_meta(path: &Path, meta: &SessionLockMeta) -> io::Result<()> {
    let tmp = path.with_extension("lock.tmp");
    let json = serde_json::to_vec(meta).map_err(io::Error::other)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, path)
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn now_iso() -> String {
    // 本地时间 RFC3339（与 runtime::business::session::types::now_iso 一致格式）。
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // 简化：用 unix 秒数作为 fallback；runtime 端有完整 RFC3339，这里仅用于 lock 元数据展示。
    format!("@{}", dur.as_secs())
}

/// acquire session lock。
///
/// - 不存在 / pid 已死 → 创建并返回 Ok。
/// - pid 仍活 → 返回 `Err(LockError::HeldAlive)`，上层可调用 `force_acquire`。
pub fn acquire(session_id: &str) -> Result<SessionLock, LockError> {
    let path = lock_path(session_id);

    if let Some(meta) = read_meta(&path)? {
        if pid_alive(meta.pid) {
            return Err(LockError::HeldAlive {
                pid: meta.pid,
                created_at: meta.created_at,
                hostname: meta.hostname,
                path,
            });
        }
        // pid 已死：lock 过期，继续覆盖。
    }

    let meta = SessionLockMeta {
        pid: std::process::id(),
        created_at: now_iso(),
        hostname: hostname(),
    };
    // 确保父目录存在（首次运行 / 测试场景）。
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    write_meta(&path, &meta)?;
    Ok(SessionLock {
        path,
        released: false,
    })
}

/// 强制 acquire（覆盖已有 lock）。用户确认后调用。
pub fn force_acquire(session_id: &str) -> io::Result<SessionLock> {
    let path = lock_path(session_id);
    let meta = SessionLockMeta {
        pid: std::process::id(),
        created_at: now_iso(),
        hostname: hostname(),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    write_meta(&path, &meta)?;
    Ok(SessionLock {
        path,
        released: false,
    })
}

/// 显式释放 lock（成功后 Drop 不再删除）。
pub fn release(lock: &mut SessionLock) -> io::Result<()> {
    if lock.released {
        return Ok(());
    }
    let _ = fs::remove_file(&lock.path);
    lock.released = true;
    Ok(())
}

#[cfg(test)]
#[path = "session_lock_tests.rs"]
mod tests;
