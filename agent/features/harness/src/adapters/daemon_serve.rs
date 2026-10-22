//! RSI daemon 服务循环：UDS 控制面 + 定时触发 + 周期串行化。
//!
//! daemon 常驻（NEVER 空闲退出），停止仅经 `Stop` 控制；Timer 周期与
//! Manual 触发共用串行闸（同时只跑一个改进周期，撞车让位 Busy）。
//! 设计契约：`docs/design/02-modules/harness/03-rsi-daemon.md`。

use crate::domain::daemon_protocol::{
    encode_ack_frame, try_decode_frame, RsiControl, RsiControlAck, RsiRunState,
};
use crate::domain::task::TaskRegistry;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{oneshot, Mutex};

/// daemon 落盘端点：sock 与单实例 lock。
#[derive(Debug, Clone)]
pub struct RsiDaemonEndpoints {
    pub sock: PathBuf,
    pub lock: PathBuf,
}

impl RsiDaemonEndpoints {
    /// 默认端点：`{agents_dir}/rsi/rsi.sock` + `{agents_dir}/rsi/rsi.lock`。
    pub fn under_agents_dir(agents_dir: &Path) -> Self {
        let rsi_dir = agents_dir.join("rsi");
        Self {
            sock: rsi_dir.join("rsi.sock"),
            lock: rsi_dir.join("rsi.lock"),
        }
    }
}

/// 单实例锁：独占创建 lock 文件；drop 时释放（进程退出由 OS 兜底）。
struct DaemonLockGuard {
    path: PathBuf,
}

impl DaemonLockGuard {
    fn acquire(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, std::process::id().to_string())?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for DaemonLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// daemon 运行时共享状态（控制应答与周期执行并发访问）。
struct DaemonState {
    cycle_running: AtomicBool,
    last_cycle: Mutex<Option<String>>,
    tasks: Mutex<TaskRegistry>,
}

impl DaemonState {
    fn new(tasks: TaskRegistry) -> Self {
        Self {
            cycle_running: AtomicBool::new(false),
            last_cycle: Mutex::new(None),
            tasks: Mutex::new(tasks),
        }
    }

    async fn snapshot(&self) -> RsiRunState {
        RsiRunState {
            cycle_running: self.cycle_running.load(Ordering::SeqCst),
            last_cycle: self.last_cycle.lock().await.clone(),
        }
    }

    /// 尝试占用周期闸（CAS）：已有周期在跑返回 false。
    /// 调用方占用成功后 MUST 执行周期并经 [`Self::finish_cycle`] 复位。
    fn begin_cycle(&self) -> bool {
        self.cycle_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// 执行注册任务周期并复位闸（占用者调用）。
    async fn run_registered_cycles(&self) {
        let results = self.tasks.lock().await.run_all_cycles().await;
        let summary = results
            .iter()
            .map(|(kind, result)| match result {
                Ok(outcome) => format!("{}: {}", kind.as_str(), outcome.summary),
                Err(error) => format!("{}: 失败（{}）", kind.as_str(), error.message()),
            })
            .collect::<Vec<_>>()
            .join("; ");
        *self.last_cycle.lock().await = Some(summary);
        self.cycle_running.store(false, Ordering::SeqCst);
    }

    /// 执行一轮改进周期（Timer 用：闸占用失败静默跳过）。
    async fn run_cycle_if_idle(&self) -> bool {
        if !self.begin_cycle() {
            return false;
        }
        self.run_registered_cycles().await;
        true
    }
}

/// 从连接读取一条控制指令（半帧等待，连接断开返回 None）。
async fn read_control(stream: &mut UnixStream) -> Option<RsiControl> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(chunk.get(..read).expect("read 不超过 chunk 长度"));
        match try_decode_frame::<RsiControl>(&buffer) {
            Ok(Some((control, _consumed))) => return Some(control),
            Ok(None) => continue,
            Err(error) => {
                log::warn!(target: crate::LOG_TARGET, "控制帧解析失败：{}", error.message());
                return None;
            }
        }
    }
}

/// 分发控制指令并写回应答帧；返回 ack 供 Stop 检测。
async fn dispatch_control(state: &Arc<DaemonState>, control: RsiControl) -> RsiControlAck {
    match control {
        RsiControl::Trigger { request_id } => {
            // 同步 CAS 占用周期闸：决断与置位同点完成，两个紧邻 Trigger
            // NEVER 都拿到 Started（spawn 内异步置位存在竞态窗口）。
            if state.begin_cycle() {
                let state = Arc::clone(state);
                tokio::spawn(async move {
                    state.run_registered_cycles().await;
                });
                RsiControlAck::Started { request_id }
            } else {
                RsiControlAck::Busy { request_id }
            }
        }
        RsiControl::Status => RsiControlAck::Status {
            state: state.snapshot().await,
        },
        RsiControl::Stop => RsiControlAck::Stopped,
    }
}

/// 运行 daemon：绑定 sock、accept 控制连接（每连接一指令一应答）、
/// 按 interval 定时触发周期；`Stop` 指令或 shutdown 信号触发优雅退出。
pub async fn run_rsi_daemon(
    endpoints: RsiDaemonEndpoints,
    tasks: TaskRegistry,
    interval: Duration,
    mut shutdown: oneshot::Receiver<()>,
) -> io::Result<()> {
    let _lock = DaemonLockGuard::acquire(&endpoints.lock)?;
    if let Some(parent) = endpoints.sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&endpoints.sock);
    let listener = UnixListener::bind(&endpoints.sock)?;
    log::info!(target: crate::LOG_TARGET, "rsi daemon 监听 {}", endpoints.sock.display());

    let state = Arc::new(DaemonState::new(tasks));
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // 丢弃 interval 的立即首拍：定时语义为「每 interval 一次」，
    // 启动不自动跑一轮（Manual 与 Timer 的行为边界由测试锁定）。
    ticker.tick().await;
    let stop_notify = Arc::new(tokio::sync::Notify::new());
    let mut shutdown_released = false;

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((mut stream, _)) = accepted else { continue };
                let state = Arc::clone(&state);
                let stop_notify = Arc::clone(&stop_notify);
                tokio::spawn(async move {
                    if let Some(control) = read_control(&mut stream).await {

                        let ack = dispatch_control(&state, control).await;
                        if stream.write_all(&encode_ack_frame(&ack)).await.is_ok()
                            && matches!(ack, RsiControlAck::Stopped)
                        {
                            stop_notify.notify_one();
                        }
                    }
                });
            }
            _ = ticker.tick() => {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    state.run_cycle_if_idle().await;
                });
            }
            _ = stop_notify.notified() => break,
            _ = &mut shutdown => {
                shutdown_released = true;
                break;
            }
        }
    }
    let _ = shutdown_released;
    let _ = std::fs::remove_file(&endpoints.sock);
    log::info!(target: crate::LOG_TARGET, "rsi daemon 已停止");
    Ok(())
}
