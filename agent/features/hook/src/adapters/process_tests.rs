use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::*;

fn request(command: impl Into<String>) -> ProcessRequest {
    ProcessRequest {
        command: command.into(),
        cwd: std::env::current_dir().expect("读取测试 cwd"),
        env: HashMap::new(),
        stdin: Vec::new(),
        timeout: Duration::from_secs(2),
        output_limit: 64,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn normal_exit_preserves_status_and_bounded_output() {
    let output = ProcessDriver
        .execute(
            request("printf 'hello'; printf 'warning' >&2; exit 7"),
            &CancellationToken::new(),
        )
        .await
        .expect("正常退出应返回机械执行结果");

    assert_eq!(output.exit_code, Some(7));
    assert_eq!(output.stdout, b"hello");
    assert_eq!(output.stderr, b"warning");
    assert!(!output.stdout_truncated);
    assert!(!output.stderr_truncated);
}

#[cfg(unix)]
#[tokio::test]
async fn large_stdout_and_stderr_are_drained_and_truncated_without_deadlock() {
    let output = ProcessDriver
        .execute(
            request("yes o | head -c 200000 & yes e | head -c 200000 >&2 & wait"),
            &CancellationToken::new(),
        )
        .await
        .expect("并发大输出应完成而不是因管道反压死锁");

    assert_eq!(output.exit_code, Some(0));
    assert_eq!(output.stdout.len(), 64);
    assert_eq!(output.stderr.len(), 64);
    assert!(output.stdout_truncated);
    assert!(output.stderr_truncated);
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(unix)]
async fn wait_for_file(path: &Path) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(contents) = tokio::fs::read_to_string(path).await {
                if contents.lines().count() >= 2 {
                    break contents;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("PID marker 应在上限内出现")
}

#[cfg(unix)]
async fn assert_process_gone(pid: u32) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while process_exists(pid) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("进程 {pid} 在回收返回后仍存活"));
}

#[cfg(unix)]
fn parse_pids(contents: &str) -> (u32, u32) {
    let mut lines = contents.lines();
    let shell = lines
        .next()
        .expect("shell pid")
        .parse()
        .expect("shell pid 数字");
    let child = lines
        .next()
        .expect("child pid")
        .parse()
        .expect("child pid 数字");
    (shell, child)
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_reaps_shell_and_descendant_processes() {
    let temp = tempfile::tempdir().expect("创建测试目录");
    let marker = temp.path().join("timeout-pids");
    let command = format!(
        "sleep 30 & child=$!; printf '%s\\n%s\\n' $$ $child > '{}'; wait",
        marker.display()
    );
    let mut request = request(command);
    request.timeout = Duration::from_millis(200);

    let marker_reader = tokio::spawn({
        let marker = marker.clone();
        async move { wait_for_file(&marker).await }
    });
    let failure = ProcessDriver
        .execute(request, &CancellationToken::new())
        .await
        .expect_err("应触发 timeout 回收");
    let (shell, child) = parse_pids(&marker_reader.await.expect("读取 PID marker"));

    assert_eq!(failure.kind, ProcessFailureKind::Timeout);
    assert_process_gone(shell).await;
    assert_process_gone(child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_reaps_shell_and_descendant_processes() {
    let temp = tempfile::tempdir().expect("创建测试目录");
    let marker = temp.path().join("cancel-pids");
    let command = format!(
        "sleep 30 & child=$!; printf '%s\\n%s\\n' $$ $child > '{}'; wait",
        marker.display()
    );
    let request = request(command);
    let cancellation = CancellationToken::new();
    let running = tokio::spawn({
        let cancellation = cancellation.clone();
        async move { ProcessDriver.execute(request, &cancellation).await }
    });

    let pids = wait_for_file(&marker).await;
    cancellation.cancel();
    let failure = running
        .await
        .expect("ProcessDriver task")
        .expect_err("应触发 cancel 回收");
    let (shell, child) = parse_pids(&pids);

    assert_eq!(failure.kind, ProcessFailureKind::Cancelled);
    assert_process_gone(shell).await;
    assert_process_gone(child).await;
}

#[cfg(unix)]
#[tokio::test]
async fn term_ignoring_process_is_escalated_to_kill_and_reaped() {
    let temp = tempfile::tempdir().expect("创建测试目录");
    let marker = temp.path().join("kill-pids");
    let command = format!(
        "trap '' TERM; sh -c 'trap \"\" TERM; while :; do sleep 1; done' & child=$!; printf '%s\\n%s\\n' $$ $child > '{}'; wait",
        marker.display()
    );
    let mut request = request(command);
    request.timeout = Duration::from_millis(200);

    let marker_reader = tokio::spawn({
        let marker = marker.clone();
        async move { wait_for_file(&marker).await }
    });
    let failure = tokio::time::timeout(
        Duration::from_secs(3),
        ProcessDriver.execute(request, &CancellationToken::new()),
    )
    .await
    .expect("TERM grace 后必须升级 KILL")
    .expect_err("应触发 timeout 回收");
    let (shell, child) = parse_pids(&marker_reader.await.expect("读取 PID marker"));

    assert_eq!(failure.kind, ProcessFailureKind::Timeout);
    assert_process_gone(shell).await;
    assert_process_gone(child).await;
}
