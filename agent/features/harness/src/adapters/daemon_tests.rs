//! daemon serve/client 集成测试（UDS 往返、串行闸、Timer、优雅停止）。

use crate::adapters::daemon_client::send_control;
use crate::adapters::daemon_serve::{run_rsi_daemon, RsiDaemonEndpoints};
use crate::domain::daemon_protocol::{RsiControl, RsiControlAck};
use crate::domain::errors::HarnessError;
use crate::domain::task::{CycleOutcome, RsiTask, TaskKind, TaskRegistry};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

fn endpoints_under(temp: &std::path::Path) -> RsiDaemonEndpoints {
    RsiDaemonEndpoints::under_agents_dir(temp)
}

/// 快任务桩：立即完成并递增计数。
struct FastTask {
    calls: AtomicU32,
}

#[async_trait]
impl RsiTask for FastTask {
    async fn run_cycle(&mut self) -> Result<CycleOutcome, HarnessError> {
        let calls = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(CycleOutcome {
            summary: format!("快周期 #{calls}"),
        })
    }
}

/// 慢任务桩：周期内 sleep，制造 Busy 窗口。
struct SlowTask;

#[async_trait]
impl RsiTask for SlowTask {
    async fn run_cycle(&mut self) -> Result<CycleOutcome, HarnessError> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(CycleOutcome {
            summary: "慢周期完成".to_string(),
        })
    }
}

async fn spawn_daemon(
    temp: &std::path::Path,
    tasks: TaskRegistry,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    let endpoints = endpoints_under(temp);
    // shutdown_tx 交给 daemon 的 Stop 控制退出；此处 forget 保持存活，
    // 立即 drop 会让 daemon 在 bind 后立即收到 shutdown 而退出。
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    std::mem::forget(shutdown_tx);
    let handle = tokio::spawn(async move {
        if let Err(error) = run_rsi_daemon(endpoints, tasks, interval, shutdown_rx).await {
            eprintln!("[daemon-test] run_rsi_daemon 失败: {error}");
        }
    });
    // 等 sock 就绪
    let sock = endpoints_under(temp).sock;
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    handle
}

/// Trigger→Status→Stop 全链路往返。
#[tokio::test]
async fn trigger_status_stop_round_trip() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut tasks = TaskRegistry::new();
    tasks
        .register(
            TaskKind::HarnessRsi,
            Box::new(FastTask {
                calls: AtomicU32::new(0),
            }),
        )
        .expect("register");
    let sock = endpoints_under(temp.path()).sock;
    let daemon = spawn_daemon(temp.path(), tasks, Duration::from_secs(3600)).await;

    let started = send_control(
        &sock,
        &RsiControl::Trigger {
            request_id: "r1".into(),
        },
    )
    .await
    .expect("Trigger 应答");
    assert!(matches!(started, RsiControlAck::Started { .. }));

    // 等周期完成（快任务）
    let mut state = None;
    for _ in 0..100 {
        if let RsiControlAck::Status { state: snapshot } = send_control(&sock, &RsiControl::Status)
            .await
            .expect("Status 应答")
        {
            if snapshot.last_cycle.is_some() {
                state = Some(snapshot);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let state = state.expect("周期必须完成并在 Status 可查");
    assert!(!state.cycle_running);
    assert!(state.last_cycle.unwrap().contains("harness-rsi"));

    let stopped = send_control(&sock, &RsiControl::Stop)
        .await
        .expect("Stop 应答");
    assert!(matches!(stopped, RsiControlAck::Stopped));
    tokio::time::timeout(Duration::from_secs(2), daemon)
        .await
        .expect("daemon 必须优雅退出")
        .expect("join");
}

/// 周期运行中 Trigger 让位 Busy（串行闸，NEVER 并行周期）。
#[tokio::test]
async fn trigger_returns_busy_while_cycle_running() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut tasks = TaskRegistry::new();
    tasks
        .register(TaskKind::HarnessRsi, Box::new(SlowTask))
        .expect("register");
    let sock = endpoints_under(temp.path()).sock;
    let daemon = spawn_daemon(temp.path(), tasks, Duration::from_secs(3600)).await;

    let started = send_control(
        &sock,
        &RsiControl::Trigger {
            request_id: "r1".into(),
        },
    )
    .await
    .expect("首个 Trigger 必须接受");
    assert!(matches!(started, RsiControlAck::Started { .. }));

    // 慢周期窗口内再次 Trigger → Busy
    let busy = send_control(
        &sock,
        &RsiControl::Trigger {
            request_id: "r2".into(),
        },
    )
    .await
    .expect("第二个 Trigger 应答");
    assert!(matches!(busy, RsiControlAck::Busy { .. }));

    let _ = send_control(&sock, &RsiControl::Stop).await;
    tokio::time::timeout(Duration::from_secs(3), daemon)
        .await
        .expect("daemon 必须优雅退出")
        .expect("join");
}

/// Timer 定时触发周期（无需 Manual）。
#[tokio::test]
async fn timer_fires_cycle_without_manual_trigger() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut tasks = TaskRegistry::new();
    tasks
        .register(
            TaskKind::HarnessRsi,
            Box::new(FastTask {
                calls: AtomicU32::new(0),
            }),
        )
        .expect("register");
    let sock = endpoints_under(temp.path()).sock;
    let daemon = spawn_daemon(temp.path(), tasks, Duration::from_millis(50)).await;

    let mut fired = false;
    for _ in 0..200 {
        if let RsiControlAck::Status { state } = send_control(&sock, &RsiControl::Status)
            .await
            .expect("Status")
        {
            if state.last_cycle.is_some() {
                fired = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(fired, "Timer 必须自动触发周期");

    let _ = send_control(&sock, &RsiControl::Stop).await;
    tokio::time::timeout(Duration::from_secs(2), daemon)
        .await
        .expect("daemon 必须优雅退出")
        .expect("join");
}

/// daemon 未启动时客户端报 Unavailable（可重拉起）。
#[tokio::test]
async fn client_reports_unavailable_when_daemon_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sock = endpoints_under(temp.path()).sock;
    let error = send_control(&sock, &RsiControl::Status)
        .await
        .expect_err("未启动必须失败");
    assert_eq!(error.category(), share::error::ErrorCategory::Unavailable);
}
