//! 后台进程领域模型（tool call 统一后台进程模型）。
//!
//! 所有 tool call 派发即登记为后台进程：前台等待只是快路径视图，
//! 超过阈值自动转后台（占位结果 + 异步回注）。本模块只维护任务监督
//! 视角的状态机与事实；逐 call 的执行事实（取消协议、重启恢复）仍由
//! Context 的 `ToolCallReceiptData` 承担，两者以 `ToolCallIdentityData`
//! 关联。任务生命周期随 CLI 进程终止（重启即失效），不做 daemon 化。
//! 详见 `docs/design/02-modules/runtime/09-background-tasks.md`。

use std::fmt;
use std::time::SystemTime;

use context::ToolCallIdentityData;
use serde::{Deserialize, Serialize};
use share::ids::BackgroundProcessId;

/// 后台进程终态种类（任务监督视角；与 receipt 终态对齐但独立维护）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundProcessTerminalKind {
    /// 执行成功完成（快路径或后台）。
    Success,
    /// 执行失败。
    Failure,
    /// 转后台快照 deadline 到期，按硬超时收敛。
    TimedOut,
    /// 经后台进程工具的 stop 请求停止。
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

/// 后台进程状态机（单调推进，禁止回退）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundProcessState {
    /// 已派发登记，前台等待中（快路径窗口内）。
    ForegroundWaiting,
    /// 超阈值转后台：执行仍在进行，占位结果已发布。
    Backgrounded {
        backgrounded_at: SystemTime,
        /// 转后台时三重 deadline 最早值快照，由监督器继续生效。
        deadline_snapshot: Option<SystemTime>,
    },
    /// 终态。
    Terminal(BackgroundProcessTerminalKind),
}

/// 后台进程记录（跨 Run 的任务监督事实）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessRecord {
    pub task_id: BackgroundProcessId,
    pub identity: ToolCallIdentityData,
    pub invocation_summary: String,
    pub state: BackgroundProcessState,
    /// 进程开始时刻（工具派发时刻，含前台等待段；旧快照字段名为
    /// `created_at`，经 serde alias 兼容读取）。
    #[serde(alias = "created_at")]
    pub started_at: SystemTime,
    /// 首次进入终态的时刻（时长冻结依据；旧快照缺失时为 None）。
    #[serde(default)]
    pub finished_at: Option<SystemTime>,
    /// 任务日志文件路径（#1890 输出直绑；直绑派发时创建，logs 查询的
    /// 文件真相源。非直绑工具由 runtime 终态兜底写入。旧快照缺失时
    /// 为 None——查询回退终态文本）。
    #[serde(default)]
    pub log_file: Option<std::path::PathBuf>,
    /// 直绑任务标志（#1890）：stdout/stderr 由子进程直写日志文件
    /// （正文已在文件内，终态 append 不回写正文，避免双写）；建档
    /// 非直绑（非流式 / Agent 兜底）为 false——终态文本 append 是其
    /// 唯一落盘通道。旧快照缺失时 false（回退 append 全文）。
    #[serde(default)]
    pub log_direct: bool,
}

/// 状态推进结果：推进后的记录与是否发生变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundProcessAdvance {
    pub record: BackgroundProcessRecord,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackgroundProcessTransitionError {
    #[error("后台进程状态转换非法：{from:?} -> {to:?}")]
    InvalidTransition {
        from: &'static str,
        to: &'static str,
    },
    #[error("后台进程已终态，禁止覆盖：{task_id}")]
    TerminalConflict { task_id: String },
}

impl BackgroundProcessState {
    fn phase_name(&self) -> &'static str {
        match self {
            BackgroundProcessState::ForegroundWaiting => "ForegroundWaiting",
            BackgroundProcessState::Backgrounded { .. } => "Backgrounded",
            BackgroundProcessState::Terminal(_) => "Terminal",
        }
    }
}

impl BackgroundProcessRecord {
    /// 派发即登记：任务记录从 ForegroundWaiting 起步。
    ///
    /// `started_at` 由调用方传入工具派发时刻（转后台登记时回填派发
    /// 时刻，使时长覆盖前台等待段）。
    pub fn dispatch(
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        started_at: SystemTime,
    ) -> Self {
        Self {
            task_id: BackgroundProcessId::new_v7(),
            identity,
            invocation_summary: invocation_summary.into(),
            state: BackgroundProcessState::ForegroundWaiting,
            started_at,
            finished_at: None,
            log_file: None,
            log_direct: false,
        }
    }

    /// 直绑派发（#1890 输出直绑）：进程 id 前移到派发时生成（spawn 前
    /// 任务日志文件名与账本 id 必须同源），文件路径随记录入账。
    pub fn dispatch_direct(
        task_id: BackgroundProcessId,
        log_file: std::path::PathBuf,
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        started_at: SystemTime,
    ) -> Self {
        Self {
            task_id,
            identity,
            invocation_summary: invocation_summary.into(),
            state: BackgroundProcessState::ForegroundWaiting,
            started_at,
            finished_at: None,
            log_file: Some(log_file),
            log_direct: true,
        }
    }

    /// 建档非直绑派发（#1890 终态兜底）：非流式 / Agent 任务转后台时
    /// 建文件但正文不经子进程直写——终态文本由 runtime append（唯一
    /// 写入方）。
    pub fn dispatch_indirect_log(
        task_id: BackgroundProcessId,
        log_file: std::path::PathBuf,
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        started_at: SystemTime,
    ) -> Self {
        Self {
            task_id,
            identity,
            invocation_summary: invocation_summary.into(),
            state: BackgroundProcessState::ForegroundWaiting,
            started_at,
            finished_at: None,
            log_file: Some(log_file),
            log_direct: false,
        }
    }

    /// 固化完成时刻：仅终态记录生效，且首次固化后不再覆盖（幂等）。
    ///
    /// 时长统计以本时刻冻结，避免查询时刻随墙钟推移导致终态时长
    /// 持续增长。
    pub fn mark_finished(&mut self, at: SystemTime) {
        if self.is_terminal() && self.finished_at.is_none() {
            self.finished_at = Some(at);
        }
    }

    pub fn is_backgrounded(&self) -> bool {
        matches!(self.state, BackgroundProcessState::Backgrounded { .. })
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, BackgroundProcessState::Terminal(_))
    }

    pub fn terminal_kind(&self) -> Option<BackgroundProcessTerminalKind> {
        match &self.state {
            BackgroundProcessState::Terminal(kind) => Some(kind.clone()),
            _ => None,
        }
    }

    /// 单调推进：ForegroundWaiting → Backgrounded / Terminal；
    /// Backgrounded → Terminal；相同状态幂等；Terminal 禁止覆盖。
    pub fn advance(
        self,
        next: BackgroundProcessState,
    ) -> Result<BackgroundProcessAdvance, BackgroundProcessTransitionError> {
        if self.state == next {
            return Ok(BackgroundProcessAdvance {
                record: self,
                changed: false,
            });
        }
        let from = self.state.phase_name();
        let to = next.phase_name();
        let allowed = match (&self.state, &next) {
            (
                BackgroundProcessState::ForegroundWaiting,
                BackgroundProcessState::Backgrounded { .. },
            )
            | (BackgroundProcessState::ForegroundWaiting, BackgroundProcessState::Terminal(_))
            | (BackgroundProcessState::Backgrounded { .. }, BackgroundProcessState::Terminal(_)) => {
                true
            }
            // 重复转后台幂等：已 Backgrounded 时忽略新时间戳（首次数据为准）。
            // Backgrounded 携带时间字段，全等比较在两次 now() 间天然不稳定。
            (
                BackgroundProcessState::Backgrounded { .. },
                BackgroundProcessState::Backgrounded { .. },
            ) => {
                return Ok(BackgroundProcessAdvance {
                    record: self,
                    changed: false,
                });
            }
            (BackgroundProcessState::Terminal(_), _) => {
                return Err(BackgroundProcessTransitionError::TerminalConflict {
                    task_id: self.task_id.as_str().to_string(),
                });
            }
            _ => false,
        };
        if !allowed {
            return Err(BackgroundProcessTransitionError::InvalidTransition { from, to });
        }
        Ok(BackgroundProcessAdvance {
            record: BackgroundProcessRecord {
                state: next,
                ..self
            },
            changed: true,
        })
    }
}

impl fmt::Display for BackgroundProcessTerminalKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            BackgroundProcessTerminalKind::Success => "success",
            BackgroundProcessTerminalKind::Failure => "failure",
            BackgroundProcessTerminalKind::TimedOut => "timed_out",
            BackgroundProcessTerminalKind::Stopped => "stopped",
            BackgroundProcessTerminalKind::Invalidated { .. } => "invalidated",
        };
        formatter.write_str(text)
    }
}

#[cfg(test)]
#[path = "background_process_tests.rs"]
mod tests;
