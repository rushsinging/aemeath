//! CLI 端 session lock 包装（#636 D3）。
//!
//! 在 chat 启动时 acquire session lock，防止多实例并发操作同一 session。
//! 冲突时在终端提示用户决定是否强制接管。

use std::io::{self, Write};

use sdk::session_lock::{self, LockError, SessionLock};

/// acquire 失败时的错误。
#[derive(Debug)]
pub enum AcquireError {
    /// 用户选择不接管，或 quiet 模式下检测到冲突。
    Denied,
    /// 其他 IO / 解析错误。
    Other(String),
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcquireError::Denied => write!(f, "session lock denied by user / quiet mode"),
            AcquireError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<LockError> for AcquireError {
    fn from(e: LockError) -> Self {
        AcquireError::Other(e.to_string())
    }
}

impl From<std::io::Error> for AcquireError {
    fn from(e: std::io::Error) -> Self {
        AcquireError::Other(e.to_string())
    }
}

/// acquire session lock，冲突时提示用户决定是否强制接管。
///
/// - `quiet`：非交互模式（如 `-q`）。冲突时直接 Denied，不提示。
pub fn try_acquire_or_prompt(session_id: &str, quiet: bool) -> Result<SessionLock, AcquireError> {
    match session_lock::acquire(session_id) {
        Ok(lock) => Ok(lock),
        Err(LockError::HeldAlive {
            pid,
            created_at,
            hostname,
            ..
        }) => {
            if quiet {
                eprintln!(
                    "⚠️  Session {session_id} 正被 PID {pid}（启动于 {created_at} on {hostname}）占用。\n\
                     非交互模式下无法提示接管，退出。请关闭占用进程后重试，或手动删除 lock 文件。"
                );
                return Err(AcquireError::Denied);
            }
            // 交互模式：stderr 提示 + stdin 读取 y/N
            eprintln!(
                "⚠️  Session {session_id} 正被 PID {pid}（启动于 {created_at} on {hostname}）占用。\n\
                 强制接管会覆盖对方的 lock（可能造成数据冲突）。是否继续？[y/N] "
            );
            let _ = io::stderr().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                return Err(AcquireError::Denied);
            }
            if input.trim().eq_ignore_ascii_case("y") {
                session_lock::force_acquire(session_id)
                    .map_err(|e| AcquireError::Other(e.to_string()))
            } else {
                Err(AcquireError::Denied)
            }
        }
        Err(other) => Err(AcquireError::from(other)),
    }
}
