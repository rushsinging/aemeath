use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::*;

struct ParentEnvironmentGuard {
    key: String,
    previous: Option<std::ffi::OsString>,
}

impl ParentEnvironmentGuard {
    fn set(key: String, value: &str) -> Self {
        let previous = std::env::var_os(&key);
        std::env::set_var(&key, value);
        Self { key, previous }
    }
}

impl Drop for ParentEnvironmentGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(&self.key, value),
            None => std::env::remove_var(&self.key),
        }
    }
}

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

#[cfg(not(unix))]
#[tokio::test]
async fn unsupported_platform_returns_typed_failure_without_running_command() {
    let failure = ProcessDriver
        .execute(
            request("this-command-must-not-run"),
            &CancellationToken::new(),
        )
        .await
        .expect_err("non-Unix 平台必须显式拒绝 Hook 命令执行");

    assert_eq!(failure.kind, ProcessFailureKind::Unsupported);
    assert_eq!(failure.message, "当前平台不支持 Hook 命令执行");
}

#[cfg(unix)]
#[tokio::test]
async fn child_starts_new_session_without_controlling_terminal() {
    let output = ProcessDriver
        .execute(
            request("python3 - <<'PY'\nimport os\ntry:\n    tty = open('/dev/tty', 'rb')\nexcept OSError:\n    tty_open = 0\nelse:\n    tty.close()\n    tty_open = 1\nprint(f'{os.getpgid(0)} {os.getsid(0)} {tty_open}')\nPY"),
            &CancellationToken::new(),
        )
        .await
        .expect("session probe should run");

    let stdout = String::from_utf8(output.stdout).expect("session probe utf8");
    let fields: Vec<&str> = stdout.split_whitespace().collect();
    assert_eq!(fields.len(), 3, "unexpected probe output: {stdout:?}");
    assert_eq!(
        fields[0], fields[1],
        "hook process group must own its session"
    );
    assert_eq!(fields[2], "0", "hook child must not have a controlling tty");
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
async fn child_sees_only_request_environment() {
    let inherited_key = format!("AEMEATH_HOOK_UNAPPROVED_{}", uuid::Uuid::new_v4().simple());
    let _guard = ParentEnvironmentGuard::set(inherited_key.clone(), "parent-secret");
    let approved_key = "AEMEATH_HOOK_TEST_APPROVED";
    let mut process_request = request(format!(
        "printf '%s|%s' \"${{{inherited_key}-missing}}\" \"${{{approved_key}-missing}}\""
    ));
    process_request
        .env
        .insert(approved_key.to_string(), "approved-value".to_string());

    let output = ProcessDriver
        .execute(process_request, &CancellationToken::new())
        .await
        .expect("隔离环境下命令应正常退出");

    assert_eq!(output.stdout, b"missing|approved-value");
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
async fn shell_exit_with_background_holder_returns_promptly_and_reaps_group() {
    let temp = tempfile::tempdir().expect("创建测试目录");
    let marker = temp.path().join("bg-pids");
    // shell 立即退出（exit 0），但后台 job 残留并持有 stdout/stderr 管道写端：
    // 修复前 read_bounded 等 EOF 会卡满 timeout（只能由 timeout 路径杀进程组）；
    // 修复后 shell 退出即应探测并回收进程组残留，快速正常返回。
    let command = format!(
        "sleep 30 & child=$!; printf '%s\\n%s\\n' $$ $child > '{}'; exit 0",
        marker.display()
    );
    let mut request = request(command);
    request.timeout = Duration::from_secs(10);
    let marker_reader = tokio::spawn({
        let marker = marker.clone();
        async move { wait_for_file(&marker).await }
    });
    let started = std::time::Instant::now();
    let output = ProcessDriver
        .execute(request, &CancellationToken::new())
        .await
        .expect("shell 已退出，残留后台 job 应被回收后正常返回");
    let elapsed = started.elapsed();
    let (shell, child) = parse_pids(&marker_reader.await.expect("读取 PID marker"));

    assert_eq!(output.exit_code, Some(0));
    assert!(
        elapsed < Duration::from_secs(3),
        "修复前残留 job 持有管道写端会卡满 timeout；实际耗时 {elapsed:?}"
    );
    assert_process_gone(child).await;
    assert_process_gone(shell).await;
}

#[cfg(unix)]
#[tokio::test]
async fn empty_stdin_is_dev_null_not_pipe() {
    // 探针：fd 0 为字符设备（/dev/null）输出 char，为管道输出 fifo。
    let probe = "[ -c /dev/stdin ] && echo char || echo fifo";
    let output = ProcessDriver
        .execute(request(probe), &CancellationToken::new())
        .await
        .expect("空 stdin 应使用 /dev/null");
    assert_eq!(output.stdout, b"char\n");

    let mut with_input = request(probe);
    with_input.stdin = b"{}".to_vec();
    let output = ProcessDriver
        .execute(with_input, &CancellationToken::new())
        .await
        .expect("非空 stdin 应使用管道");
    assert_eq!(output.stdout, b"fifo\n");
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
