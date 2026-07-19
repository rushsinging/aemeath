//! Scripted fake（仅测试）—— 按入队顺序回放每步结果。
//!
//! 从 `dispatcher.rs` 拆出：测试用脚本化 `Executor` 实现，避免依赖真实难制造的
//! Wait/IO 故障。生产代码不含本文件（`#[cfg(test)]`）。

#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::domain::subscription::HookCommand;

use super::executor::{ExecutionFault, Executor, RawExecution};

/// 测试用脚本化 Executor：按入队顺序回放，并记录每次调用。
///
/// 内部状态由 `Arc` 共享，因此 `clone()` 产生共享同一回放/记录的句柄，
/// 便于把一个克隆注入 `Dispatcher`、另一个留在测试中检视。
#[derive(Debug, Clone, Default)]
pub(super) struct Scripted {
    steps: Arc<std::sync::Mutex<std::collections::VecDeque<ScriptStep>>>,
    calls: Arc<std::sync::Mutex<Vec<ScriptedCall>>>,
}

/// 单步回放脚本。
#[derive(Debug)]
pub(super) enum ScriptStep {
    /// 进程正常执行，返回原始结果。
    Ok(RawExecution),
    /// 协议级故障。
    Fault(ExecutionFault),
}

/// 一次 `execute` 调用的记录。
#[derive(Debug, Clone)]
pub(super) struct ScriptedCall {
    /// 命令字符串。
    pub command: String,
    /// 传入的 stdin JSON。
    pub stdin: serde_json::Value,
}

impl ScriptStep {
    /// 构造「退出码 + stdout」成功步（stderr 空）。
    pub(super) fn ok_exit(exit: i32, stdout: &str) -> Self {
        ScriptStep::Ok(RawExecution {
            exit_code: Some(exit),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    /// 构造 exit 0 + stdout（用于 JSON / 空输出场景）。
    pub(super) fn ok_json(stdout: &str) -> Self {
        ScriptStep::Ok(RawExecution {
            exit_code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    /// 构造「进程未正常退出」步（exit_code=None，空 stdout/stderr）。
    ///
    /// 用于验证 `classify_directive` 必须把 `exit_code=None` 分类为
    /// `MissingExitCode`（ExecutionFailed 可重试），而非按空 stdout 当 Continue。
    pub(super) fn no_exit_code() -> Self {
        ScriptStep::Ok(RawExecution {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    /// 构造协议级故障步。
    pub(super) fn fault(kind: ExecutionFault) -> Self {
        ScriptStep::Fault(kind)
    }
}

impl Scripted {
    /// 按给定顺序入队回放步骤。
    pub(super) fn from_steps(steps: impl IntoIterator<Item = ScriptStep>) -> Self {
        Self {
            steps: Arc::new(std::sync::Mutex::new(steps.into_iter().collect())),
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// 返回 `execute` 被调用次数。
    pub(super) fn call_count(&self) -> usize {
        self.calls.lock().expect("scripted calls lock").len()
    }

    /// 返回所有调用记录（命令 + stdin）。
    pub(super) fn calls(&self) -> Vec<ScriptedCall> {
        self.calls.lock().expect("scripted calls lock").clone()
    }

    /// 返回按序的命令字符串。
    pub(super) fn commands(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("scripted calls lock")
            .iter()
            .map(|c| c.command.clone())
            .collect()
    }
}

#[async_trait]
impl Executor for Scripted {
    async fn execute(
        &self,
        command: &HookCommand,
        stdin: &serde_json::Value,
        _timeout: Duration,
        _cancellation: &CancellationToken,
    ) -> Result<RawExecution, ExecutionFault> {
        self.calls
            .lock()
            .expect("scripted calls lock")
            .push(ScriptedCall {
                command: command.command.clone(),
                stdin: stdin.clone(),
            });
        match self.steps.lock().expect("scripted steps lock").pop_front() {
            Some(ScriptStep::Ok(raw)) => Ok(raw),
            Some(ScriptStep::Fault(f)) => Err(f),
            None => panic!("Scripted 执行器步骤耗尽：没有更多入队步骤"),
        }
    }
}
