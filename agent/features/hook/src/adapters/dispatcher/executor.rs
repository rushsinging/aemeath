//! Executor port + 生产适配（`ProcessDriverExecutor`）。
//!
//! 从 `dispatcher.rs` 拆出：本模块仅承载「单次命令执行」的最窄语义抽象，
//! 不含任何编排逻辑（重试 / 聚合 / 短路 / StopFailure 派发等全部留在 dispatcher.rs）。
//!
//! - `Executor` trait：dispatcher 通过本端口执行单条命令；
//! - `RawExecution` / `ExecutionFault`：执行结果与协议级故障类型；
//! - `ProcessDriverExecutor`：生产适配 `ProcessDriver`（`adapters/process.rs`）。
//!
//! 全部 `pub(crate)` detail，**NEVER** 进入 crate 稳定 façade。

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::domain::subscription::HookCommand;
use crate::ports::CancellationSignal;

#[cfg(any(not(unix), test))]
use crate::adapters::process::UNSUPPORTED_PLATFORM_MESSAGE;
use crate::adapters::process::{
    ProcessDriver, ProcessFailure, ProcessFailureKind, ProcessRequest, DEFAULT_OUTPUT_LIMIT,
};

/// 单次命令执行的原始机械结果（已 drain + 截断）。
#[derive(Debug, Clone)]
pub(crate) struct RawExecution {
    /// 进程退出码（进程未正常退出时为 None）。
    pub(crate) exit_code: Option<i32>,
    /// stdout（已截断）。
    pub(crate) stdout: String,
    /// stderr（已截断）。
    pub(crate) stderr: String,
}

/// 单次执行的协议级故障（ExecutionFailed 可重试路径）。
///
/// 与业务 Block（`HookReason`）严格区分：业务 Block 永不重试，
/// 本枚举中的可恢复执行故障按 Dispatcher 注入的 execution policy 重试。
/// `Unsupported` 是永久性平台能力缺失，`Cancelled` 是调用方终止；二者均立即结束，
/// 不进入重试循环，但仍保留一次 typed execution 明细。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionFault {
    #[cfg(test)]
    Spawn,
    SpawnWithDetail(String),
    Io,
    Wait,
    Timeout,
    #[cfg(any(not(unix), test))]
    Unsupported,
    Cancelled,
}

impl ExecutionFault {
    pub(crate) fn message(&self) -> String {
        match self {
            #[cfg(test)]
            Self::Spawn => "hook 子进程启动失败".to_string(),
            Self::SpawnWithDetail(detail) => format!("hook 子进程启动失败: {detail}"),
            Self::Io => "hook 进程管道读写失败".to_string(),
            Self::Wait => "等待 hook 子进程失败".to_string(),
            Self::Timeout => "hook 执行超时".to_string(),
            #[cfg(any(not(unix), test))]
            Self::Unsupported => UNSUPPORTED_PLATFORM_MESSAGE.to_string(),
            Self::Cancelled => "hook 执行被取消".to_string(),
        }
    }
}

/// 私有 Executor port —— 抽象单次命令执行。
///
/// dispatcher 通过本端口执行单条命令并拿到原始结果（exit code + stdout/stderr）
/// 或协议级故障；重试、分类、聚合全部由 dispatcher 编排，使本端口保持
/// 「执行一次」的最窄语义，便于测试用 `Scripted` 回放，生产用
/// `ProcessDriverExecutor` 适配 `ProcessDriver`。
#[async_trait]
pub(crate) trait Executor: Send + Sync {
    /// 执行一次命令。
    ///
    /// - `stdin` 为序列化的结构化调用 JSON（含 point、payload、session 元数据）；
    /// - `timeout` 为本次执行的超时上限（来自 subscription）；
    /// - `cancellation` 用于终止 Hook 子进程及重试等待。
    async fn execute(
        &self,
        command: &HookCommand,
        stdin: &serde_json::Value,
        cwd: &std::path::Path,
        env: &HashMap<String, String>,
        timeout: Duration,
        cancellation: &dyn CancellationSignal,
    ) -> Result<RawExecution, ExecutionFault>;
}

// ════════════════════════════════════════════════════════════
// ProcessDriverExecutor —— 生产 Executor 适配（pub(crate) detail）
// ════════════════════════════════════════════════════════════

/// 生产用 `Executor`：适配 `ProcessDriver` 的受管子进程执行。
///
/// 由 [`crate::adapters::dispatcher::Dispatcher::try_new`] 在固定基础环境白名单下
/// 内部装配，**NEVER** 对外暴露。
#[derive(Debug, Default)]
pub(crate) struct ProcessDriverExecutor {
    driver: ProcessDriver,
    env: HashMap<String, String>,
    output_limit: usize,
}

impl ProcessDriverExecutor {
    /// 创建生产 Executor：env 为 Hook adapter 提供的兼容环境投影。
    pub(crate) fn new(env: HashMap<String, String>) -> Self {
        Self {
            driver: ProcessDriver,
            env,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

#[async_trait]
impl Executor for ProcessDriverExecutor {
    async fn execute(
        &self,
        command: &HookCommand,
        stdin: &serde_json::Value,
        cwd: &std::path::Path,
        env: &HashMap<String, String>,
        timeout: Duration,
        cancellation: &dyn CancellationSignal,
    ) -> Result<RawExecution, ExecutionFault> {
        let stdin_bytes = serde_json::to_vec(stdin).unwrap_or_default();
        let request = ProcessRequest {
            command: command.command.clone(),
            cwd: cwd.to_path_buf(),
            env: self
                .env
                .iter()
                .chain(env)
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            stdin: stdin_bytes,
            timeout,
            output_limit: self.output_limit,
        };
        match self.driver.execute(request, cancellation).await {
            Ok(output) => {
                // HookExecution PL 当前不发布截断字段；ProcessDriver 仍负责 drain 与截断，
                // Dispatcher 只消费正文。
                let _output_was_truncated = output.stdout_truncated || output.stderr_truncated;
                Ok(RawExecution {
                    exit_code: output.exit_code,
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                })
            }
            Err(failure) => Err(map_process_failure(failure)),
        }
    }
}

/// 将 `ProcessFailure` 映射到 dispatcher 的 `ExecutionFault`。
fn map_process_failure(failure: ProcessFailure) -> ExecutionFault {
    match failure.kind {
        ProcessFailureKind::Spawn => ExecutionFault::SpawnWithDetail(failure.message),
        ProcessFailureKind::Io => ExecutionFault::Io,
        ProcessFailureKind::Wait => ExecutionFault::Wait,
        ProcessFailureKind::Timeout => ExecutionFault::Timeout,
        ProcessFailureKind::Cancelled => ExecutionFault::Cancelled,
        #[cfg(not(unix))]
        ProcessFailureKind::Unsupported => ExecutionFault::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_failure_preserves_os_detail_in_execution_message() {
        let fault = map_process_failure(ProcessFailure {
            kind: ProcessFailureKind::Spawn,
            message: "启动 hook 命令失败: Too many open files (os error 24)".to_string(),
        });

        assert_eq!(
            fault.message(),
            "hook 子进程启动失败: 启动 hook 命令失败: Too many open files (os error 24)"
        );
    }
}
