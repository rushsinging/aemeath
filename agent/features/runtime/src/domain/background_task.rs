//! 后台任务领域模型（tool call 统一后台任务模型）。
//!
//! 所有 tool call 派发即登记为后台任务：前台等待只是快路径视图，
//! 超过阈值自动转后台（占位结果 + 异步回注）。本模块只维护任务监督
//! 视角的状态机与事实；逐 call 的执行事实（取消协议、重启恢复）仍由
//! Context 的 `ToolCallReceiptData` 承担，两者以 `ToolCallIdentityData`
//! 关联。任务生命周期随 CLI 进程终止（重启即失效），不做 daemon 化。
//! 详见 `docs/design/02-modules/runtime/09-background-tasks.md`。

use std::fmt;
use std::time::SystemTime;

use context::ToolCallIdentityData;
use serde::{Deserialize, Serialize};
use share::ids::BackgroundTaskId;

/// 后台任务终态种类（任务监督视角；与 receipt 终态对齐但独立维护）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskTerminalKind {
    /// 执行成功完成（快路径或后台）。
    Success,
    /// 执行失败。
    Failure,
    /// 转后台快照 deadline 到期，按硬超时收敛。
    TimedOut,
    /// 经后台任务工具的 stop 请求停止。
    Stopped,
    /// 任务随进程生命周期失效（重启 / 退出）。
    Invalidated {
        reason: BackgroundInvalidationReason,
    },
}

/// 任务失效原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundInvalidationReason {
    /// CLI 进程退出（terminate drain）。
    ProcessExit,
    /// Session 恢复时发现无存活执行体。
    SessionRestored,
}

/// 后台任务状态机（单调推进，禁止回退）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskState {
    /// 已派发登记，前台等待中（快路径窗口内）。
    ForegroundWaiting,
    /// 超阈值转后台：执行仍在进行，占位结果已发布。
    Backgrounded {
        backgrounded_at: SystemTime,
        /// 转后台时三重 deadline 最早值快照，由监督器继续生效。
        deadline_snapshot: Option<SystemTime>,
    },
    /// 终态。
    Terminal(BackgroundTaskTerminalKind),
}

/// 后台任务记录（跨 Run 的任务监督事实）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskRecord {
    pub task_id: BackgroundTaskId,
    pub identity: ToolCallIdentityData,
    pub invocation_summary: String,
    pub state: BackgroundTaskState,
    pub created_at: SystemTime,
}

/// 状态推进结果：推进后的记录与是否发生变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundTaskAdvance {
    pub record: BackgroundTaskRecord,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackgroundTaskTransitionError {
    #[error("后台任务状态转换非法：{from:?} -> {to:?}")]
    InvalidTransition {
        from: &'static str,
        to: &'static str,
    },
    #[error("后台任务已终态，禁止覆盖：{task_id}")]
    TerminalConflict { task_id: String },
}

impl BackgroundTaskState {
    fn phase_name(&self) -> &'static str {
        match self {
            BackgroundTaskState::ForegroundWaiting => "ForegroundWaiting",
            BackgroundTaskState::Backgrounded { .. } => "Backgrounded",
            BackgroundTaskState::Terminal(_) => "Terminal",
        }
    }
}

impl BackgroundTaskRecord {
    /// 派发即登记：任务记录从 ForegroundWaiting 起步。
    pub fn dispatch(identity: ToolCallIdentityData, invocation_summary: impl Into<String>) -> Self {
        Self {
            task_id: BackgroundTaskId::new_v7(),
            identity,
            invocation_summary: invocation_summary.into(),
            state: BackgroundTaskState::ForegroundWaiting,
            created_at: SystemTime::now(),
        }
    }

    pub fn is_backgrounded(&self) -> bool {
        matches!(self.state, BackgroundTaskState::Backgrounded { .. })
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, BackgroundTaskState::Terminal(_))
    }

    pub fn terminal_kind(&self) -> Option<BackgroundTaskTerminalKind> {
        match &self.state {
            BackgroundTaskState::Terminal(kind) => Some(kind.clone()),
            _ => None,
        }
    }

    /// 单调推进：ForegroundWaiting → Backgrounded / Terminal；
    /// Backgrounded → Terminal；相同状态幂等；Terminal 禁止覆盖。
    pub fn advance(
        self,
        next: BackgroundTaskState,
    ) -> Result<BackgroundTaskAdvance, BackgroundTaskTransitionError> {
        if self.state == next {
            return Ok(BackgroundTaskAdvance {
                record: self,
                changed: false,
            });
        }
        let from = self.state.phase_name();
        let to = next.phase_name();
        let allowed = match (&self.state, &next) {
            (BackgroundTaskState::ForegroundWaiting, BackgroundTaskState::Backgrounded { .. })
            | (BackgroundTaskState::ForegroundWaiting, BackgroundTaskState::Terminal(_))
            | (BackgroundTaskState::Backgrounded { .. }, BackgroundTaskState::Terminal(_)) => true,
            (BackgroundTaskState::Terminal(_), _) => {
                return Err(BackgroundTaskTransitionError::TerminalConflict {
                    task_id: self.task_id.as_str().to_string(),
                });
            }
            _ => false,
        };
        if !allowed {
            return Err(BackgroundTaskTransitionError::InvalidTransition { from, to });
        }
        Ok(BackgroundTaskAdvance {
            record: BackgroundTaskRecord {
                state: next,
                ..self
            },
            changed: true,
        })
    }
}

impl fmt::Display for BackgroundTaskTerminalKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            BackgroundTaskTerminalKind::Success => "success",
            BackgroundTaskTerminalKind::Failure => "failure",
            BackgroundTaskTerminalKind::TimedOut => "timed_out",
            BackgroundTaskTerminalKind::Stopped => "stopped",
            BackgroundTaskTerminalKind::Invalidated { .. } => "invalidated",
        };
        formatter.write_str(text)
    }
}

#[cfg(test)]
#[path = "background_task_tests.rs"]
mod tests;
