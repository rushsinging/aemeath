//! 外部子进程的进程树终止与回收。
//!
//! Bash 与 Grep 等工具 spawn 的子进程都经
//! [`utils::configure_tokio_noninteractive`] 在 exec 前建立独立 session
//! （child 的 `PID = PGID = SID`），因此按负 PID 发送信号只会命中子进程
//! 自身的进程组，不会波及宿主进程。

use std::time::Duration;
use tokio::process::Command;

/// SIGTERM 后等待子进程自然退出的宽限；超时即 SIGKILL。
const TERM_GRACE: Duration = Duration::from_millis(200);

/// 终止子进程的整个进程组并完成回收（reap）。
///
/// 顺序：SIGTERM 进程组 → 有界宽限内退出则确认 → 否则 SIGKILL 进程组 →
/// 兜底 `kill()` + `wait()` 保证 child 已被回收。调用返回即代表进程树已
/// 收敛，可作为 cleanup confirmation 的依据。
pub(crate) async fn terminate_process_tree(child: &mut tokio::process::Child) {
    let child_pid = child.id();
    log::debug!(
        target: crate::LOG_TARGET,
        "child process cleanup started: pid={child_pid:?}"
    );
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // 子进程可能已自行退出（pid 已被回收），kill 对已消失进程组报错是
        // 正常竞态；丢弃其输出避免泄漏到宿主 stderr。
        let mut term_command = Command::new("kill");
        term_command
            .arg("-TERM")
            .arg(format!("-{pid}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let term_status = match utils::configure_tokio_noninteractive(&mut term_command) {
            Ok(()) => term_command.status().await,
            Err(error) => Err(error),
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "child process group SIGTERM sent: pid={} status={term_status:?}",
            pid
        );
        if tokio::time::timeout(TERM_GRACE, child.wait()).await.is_ok() {
            log::debug!(
                target: crate::LOG_TARGET,
                "child process cleanup confirmed after SIGTERM: pid={}",
                pid
            );
            return;
        }
        let mut kill_command = Command::new("kill");
        kill_command
            .arg("-KILL")
            .arg(format!("-{pid}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let kill_status = match utils::configure_tokio_noninteractive(&mut kill_command) {
            Ok(()) => kill_command.status().await,
            Err(error) => Err(error),
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "child process group SIGKILL sent: pid={} status={kill_status:?}",
            pid
        );
    }
    let child_kill = child.kill().await;
    let child_wait = child.wait().await;
    log::debug!(
        target: crate::LOG_TARGET,
        "child process cleanup terminal: pid={child_pid:?} child_kill={child_kill:?} child_wait={child_wait:?}"
    );
}
