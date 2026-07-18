//! Hook 子进程的受管执行边界。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio_util::sync::CancellationToken;

pub(crate) const DEFAULT_OUTPUT_LIMIT: usize = 8 * 1024;
const TERMINATION_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(crate) struct ProcessRequest {
    pub command: String,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub output_limit: usize,
}

#[derive(Debug)]
pub(crate) struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessFailureKind {
    Spawn,
    Io,
    Wait,
    Timeout,
    Cancelled,
    #[cfg(not(unix))]
    Unsupported,
}

#[derive(Debug)]
pub(crate) struct ProcessFailure {
    pub kind: ProcessFailureKind,
    pub message: String,
}

impl ProcessFailure {
    fn new(kind: ProcessFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProcessDriver;

#[derive(Debug)]
struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

type IoResult = Result<(BoundedOutput, BoundedOutput), ProcessFailure>;

impl ProcessDriver {
    #[cfg(unix)]
    pub(crate) async fn execute(
        &self,
        request: ProcessRequest,
        cancellation: &CancellationToken,
    ) -> Result<ProcessOutput, ProcessFailure> {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&request.command)
            .current_dir(&request.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(&request.env)
            // 仅作为 runtime/task 被强制丢弃时的直接 child 兜底；正常路径必须按进程组回收。
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);

        let mut child = command.spawn().map_err(|error| {
            ProcessFailure::new(
                ProcessFailureKind::Spawn,
                format!("启动 hook 命令失败: {error}"),
            )
        })?;
        let process_group = child
            .id()
            .ok_or_else(|| ProcessFailure::new(ProcessFailureKind::Spawn, "hook 子进程缺少 PID"))?;
        let stdin = take_pipe_or_reap(
            child.stdin.take(),
            &mut child,
            process_group,
            "hook stdin 管道不可用",
        )
        .await?;
        let stdout = take_pipe_or_reap(
            child.stdout.take(),
            &mut child,
            process_group,
            "hook stdout 管道不可用",
        )
        .await?;
        let stderr = take_pipe_or_reap(
            child.stderr.take(),
            &mut child,
            process_group,
            "hook stderr 管道不可用",
        )
        .await?;

        let mut io = Box::pin(run_io(
            stdin,
            request.stdin,
            stdout,
            stderr,
            request.output_limit,
        ));
        let deadline_sleep = tokio::time::sleep(request.timeout);
        tokio::pin!(deadline_sleep);
        let mut status: Option<ExitStatus> = None;
        let mut io_result: Option<IoResult> = None;

        loop {
            if status.is_some() && io_result.is_some() {
                let status = status.take().expect("status 已检查");
                let (stdout, stderr) = io_result.take().expect("IO result 已检查")?;
                return Ok(ProcessOutput {
                    exit_code: status.code(),
                    stdout: stdout.bytes,
                    stderr: stderr.bytes,
                    stdout_truncated: stdout.truncated,
                    stderr_truncated: stderr.truncated,
                });
            }

            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    terminate_and_reap(&mut child, process_group, status.take()).await;
                    let _ = io.await;
                    return Err(ProcessFailure::new(
                        ProcessFailureKind::Cancelled,
                        format!("hook '{}' 已取消", request.command),
                    ));
                }
                _ = &mut deadline_sleep => {
                    terminate_and_reap(&mut child, process_group, status.take()).await;
                    let _ = io.await;
                    return Err(ProcessFailure::new(
                        ProcessFailureKind::Timeout,
                        format!("hook '{}' 超时（{}毫秒）", request.command, request.timeout.as_millis()),
                    ));
                }
                result = child.wait(), if status.is_none() => {
                    match result {
                        Ok(exit_status) => status = Some(exit_status),
                        Err(error) => {
                            terminate_and_reap(&mut child, process_group, None).await;
                            let _ = io.await;
                            return Err(ProcessFailure::new(
                                ProcessFailureKind::Wait,
                                format!("等待 hook 进程失败: {error}"),
                            ));
                        }
                    }
                }
                result = &mut io, if io_result.is_none() => {
                    match result {
                        Ok(outputs) => io_result = Some(Ok(outputs)),
                        Err(error) => {
                            terminate_and_reap(&mut child, process_group, status.take()).await;
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(unix))]
    pub(crate) async fn execute(
        &self,
        _request: ProcessRequest,
        _cancellation: &CancellationToken,
    ) -> Result<ProcessOutput, ProcessFailure> {
        Err(ProcessFailure::new(
            ProcessFailureKind::Unsupported,
            "当前平台不支持可证明的 Hook 进程组回收",
        ))
    }
}

async fn run_io(
    mut stdin: ChildStdin,
    input: Vec<u8>,
    stdout: ChildStdout,
    stderr: ChildStderr,
    output_limit: usize,
) -> IoResult {
    let write = async move {
        if let Err(error) = stdin.write_all(&input).await {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(ProcessFailure::new(
                    ProcessFailureKind::Io,
                    format!("写入 hook stdin 失败: {error}"),
                ));
            }
        }
        if let Err(error) = stdin.shutdown().await {
            if error.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(ProcessFailure::new(
                    ProcessFailureKind::Io,
                    format!("关闭 hook stdin 失败: {error}"),
                ));
            }
        }
        Ok(())
    };
    let read_stdout = read_bounded(stdout, output_limit, "stdout");
    let read_stderr = read_bounded(stderr, output_limit, "stderr");
    let (write_result, stdout_result, stderr_result) =
        tokio::join!(write, read_stdout, read_stderr);
    write_result?;
    Ok((stdout_result?, stderr_result?))
}

async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
    stream: &'static str,
) -> Result<BoundedOutput, ProcessFailure> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut buffer).await.map_err(|error| {
            ProcessFailure::new(
                ProcessFailureKind::Io,
                format!("读取 hook {stream} 失败: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        let retained = limit.saturating_sub(bytes.len()).min(count);
        bytes.extend_from_slice(&buffer[..retained]);
        truncated |= retained < count;
    }
    Ok(BoundedOutput { bytes, truncated })
}

#[cfg(unix)]
async fn take_pipe_or_reap<T>(
    pipe: Option<T>,
    child: &mut Child,
    process_group: u32,
    message: &'static str,
) -> Result<T, ProcessFailure> {
    match pipe {
        Some(pipe) => Ok(pipe),
        None => {
            terminate_and_reap(child, process_group, None).await;
            Err(ProcessFailure::new(ProcessFailureKind::Spawn, message))
        }
    }
}

#[cfg(unix)]
async fn terminate_and_reap(
    child: &mut Child,
    process_group: u32,
    known_status: Option<ExitStatus>,
) {
    signal_process_group(process_group, libc::SIGTERM);
    tokio::time::sleep(TERMINATION_GRACE).await;
    signal_process_group(process_group, libc::SIGKILL);
    if known_status.is_none() {
        let _ = child.wait().await;
    }
}

#[cfg(unix)]
fn signal_process_group(process_group: u32, signal: libc::c_int) {
    unsafe {
        libc::kill(-(process_group as libc::pid_t), signal);
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
