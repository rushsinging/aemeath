//! 后台进程监督器：任务登记、状态推进、输出采集与失效收口。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use context::ToolCallIdentityData;
use share::ids::BackgroundProcessId;

use crate::application::background_process::log_file::TaskLogFile;
use crate::domain::background_process::{
    BackgroundInvalidationReason, BackgroundProcessAdvance, BackgroundProcessRecord,
    BackgroundProcessState, BackgroundProcessTerminalKind, BackgroundProcessTransitionError,
};

/// 监督中的后台进程：领域记录 + 终态结果。
///
/// 输出真相源是任务日志文件（#1890 输出直绑；record.log_file）；
/// 非直绑 / 无文件任务回退终态文本。
struct SupervisedBackgroundProcess {
    record: BackgroundProcessRecord,
    terminal_output: Option<String>,
    /// 完成事实是否已进入通知通道（take_unnotified 语义）。
    notified: bool,
    /// 子任务 cancellation token（stop 请求的取消信号源）。
    child_cancellation: tokio_util::sync::CancellationToken,
}

/// stop 请求结果（真实终态由执行体收口）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopRequestOutcome {
    /// 已发 cancel 信号；任务当前状态（终态稍后经通知/查询可见）。
    SignalSent { state: BackgroundProcessState },
    /// 任务已是终态（幂等，无副作用）。
    AlreadyTerminal(BackgroundProcessTerminalKind),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BackgroundProcessSupervisorError {
    #[error("后台进程不存在：{0}")]
    TaskNotFound(String),
    #[error("后台进程状态推进失败：{0}")]
    Transition(#[from] BackgroundProcessTransitionError),
}

/// session 级后台进程监督器。
///
/// 所有方法短临界区同步访问；异步驱动（JoinHandle 托管、完成回调）
/// 由执行流改造层挂接，本类型只维护任务事实与查询视图。
pub(crate) struct BackgroundProcessSupervisor {
    tasks: Mutex<HashMap<BackgroundProcessId, SupervisedBackgroundProcess>>,
}

impl Default for BackgroundProcessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundProcessSupervisor {
    pub(crate) fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
        }
    }

    /// 派发即登记：任务从 ForegroundWaiting 起步，返回稳定 task id。
    pub(crate) fn register(
        &self,
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        started_at: SystemTime,
    ) -> BackgroundProcessId {
        self.register_with_cancellation(
            identity,
            invocation_summary,
            tokio_util::sync::CancellationToken::new(),
            started_at,
        )
    }

    /// 派发登记（携带子任务 cancellation token，stop 请求的取消源）。
    ///
    /// `started_at` = 工具派发时刻（调用方换算传入），时长统计自该
    /// 时刻起算，覆盖转后台前的前台等待段。
    pub(crate) fn register_with_cancellation(
        &self,
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        child_cancellation: tokio_util::sync::CancellationToken,
        started_at: SystemTime,
    ) -> BackgroundProcessId {
        let task = SupervisedBackgroundProcess {
            record: BackgroundProcessRecord::dispatch(identity, invocation_summary, started_at),
            terminal_output: None,
            notified: false,
            child_cancellation,
        };
        let task_id = task.record.task_id.clone();
        self.tasks
            .lock()
            .expect("后台进程表锁中毒")
            .insert(task_id.clone(), task);
        task_id
    }

    /// 任务的日志文件路径（#1890 文件真相源；无文件任务 None）。
    pub(crate) fn log_file_of(&self, task_id: &BackgroundProcessId) -> Option<std::path::PathBuf> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        tasks
            .get(task_id)
            .and_then(|task| task.record.log_file.clone())
    }

    /// 直绑派发登记（#1890 输出直绑）：id 派发时前移生成（与任务日志
    /// 文件名同源），路径随记录入账；logs 查询直读文件区间；终态不
    /// 回写正文（子进程已直写）。
    pub(crate) fn register_direct(
        &self,
        task_id: BackgroundProcessId,
        log_file: std::path::PathBuf,
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        child_cancellation: tokio_util::sync::CancellationToken,
        started_at: SystemTime,
    ) -> BackgroundProcessId {
        let task = SupervisedBackgroundProcess {
            record: BackgroundProcessRecord::dispatch_direct(
                task_id,
                log_file,
                identity,
                invocation_summary,
                started_at,
            ),
            terminal_output: None,
            notified: false,
            child_cancellation,
        };
        let task_id = task.record.task_id.clone();
        self.tasks
            .lock()
            .expect("后台进程表锁中毒")
            .insert(task_id.clone(), task);
        task_id
    }

    /// 建档非直绑登记（#1890 终态兜底）：非流式 / Agent 任务转后台时
    /// 建文件；终态文本由 notify_terminal append（唯一落盘通道）。
    pub(crate) fn register_indirect_log(
        &self,
        task_id: BackgroundProcessId,
        log_file: std::path::PathBuf,
        identity: ToolCallIdentityData,
        invocation_summary: impl Into<String>,
        child_cancellation: tokio_util::sync::CancellationToken,
        started_at: SystemTime,
    ) -> BackgroundProcessId {
        let task = SupervisedBackgroundProcess {
            record: BackgroundProcessRecord::dispatch_indirect_log(
                task_id,
                log_file,
                identity,
                invocation_summary,
                started_at,
            ),
            terminal_output: None,
            notified: false,
            child_cancellation,
        };
        let task_id = task.record.task_id.clone();
        self.tasks
            .lock()
            .expect("后台进程表锁中毒")
            .insert(task_id.clone(), task);
        task_id
    }

    /// 是否直绑任务（终态 append 决策：直绑不回写正文）。
    pub(crate) fn is_log_direct(&self, task_id: &BackgroundProcessId) -> Option<bool> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        tasks.get(task_id).map(|task| task.record.log_direct)
    }

    /// stop 请求（#252 D10）：发 cancel 信号；真实终态由执行体收口后
    /// 经既有 finish 路径推进（幂等），本方法只请求停止并报告当前状态。
    pub(crate) fn stop_task(
        &self,
        task_id: &BackgroundProcessId,
    ) -> Result<StopRequestOutcome, BackgroundProcessSupervisorError> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let task = tasks.get(task_id).ok_or_else(|| {
            BackgroundProcessSupervisorError::TaskNotFound(task_id.as_str().to_string())
        })?;
        if let Some(kind) = task.record.terminal_kind() {
            return Ok(StopRequestOutcome::AlreadyTerminal(kind));
        }
        task.child_cancellation.cancel();
        Ok(StopRequestOutcome::SignalSent {
            state: task.record.state.clone(),
        })
    }

    /// 任务日志读取（#1890 文件真相源；#252 D12 增量游标语义不变）：
    /// - 有任务日志文件 → 文件区间读（`None` 游标 = 尾部视图，
    ///   `Some(cursor)` = 增量读取；累计写入 = 文件实际大小，跨重启
    ///   天然续读）；
    /// - 无文件（非直绑且未终态落盘 / 旧快照）→ 回退终态文本。
    ///
    /// 返回 `(文本, 读后游标, 累计写入)`；多次读取幂等（非消耗性）。
    pub(crate) fn read_task_log(
        &self,
        task_id: &BackgroundProcessId,
        cursor: Option<u64>,
        max_bytes: usize,
    ) -> Option<(String, u64, u64)> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let task = tasks.get(task_id)?;
        if let Some(log_path) = task.record.log_file.clone() {
            drop(tasks);
            let log = TaskLogFile::from_path(log_path);
            let size = log.size_bytes();
            let start = cursor.unwrap_or(size.saturating_sub(max_bytes as u64));
            let segment = log.read_range(start, max_bytes).ok()?;
            let text = String::from_utf8_lossy(&segment.bytes).into_owned();
            return Some((text, segment.next_cursor, size));
        }
        let terminal_text = task.terminal_output.clone().unwrap_or_default();
        let total = terminal_text.len() as u64;
        Some((terminal_text, total, total))
    }

    /// 转后台：推进任务状态并固化 deadline 快照。
    pub(crate) fn mark_backgrounded(
        &self,
        task_id: &BackgroundProcessId,
        deadline_snapshot: Option<SystemTime>,
    ) -> Result<(), BackgroundProcessSupervisorError> {
        self.advance(
            task_id,
            BackgroundProcessState::Backgrounded {
                backgrounded_at: SystemTime::now(),
                deadline_snapshot,
            },
        )
    }

    /// 终态推进：返回是否发生变化（重复终态幂等返回 false）。
    pub(crate) fn finish(
        &self,
        task_id: &BackgroundProcessId,
        kind: BackgroundProcessTerminalKind,
        terminal_output: Option<String>,
    ) -> Result<bool, BackgroundProcessSupervisorError> {
        let mut tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let task = tasks.get_mut(task_id).ok_or_else(|| {
            BackgroundProcessSupervisorError::TaskNotFound(task_id.as_str().to_string())
        })?;
        if terminal_output.is_some() {
            task.terminal_output = terminal_output;
        }
        let BackgroundProcessAdvance { record, changed } = task
            .record
            .clone()
            .advance(BackgroundProcessState::Terminal(kind))?;
        task.record = record;
        if changed {
            task.record.mark_finished(SystemTime::now());
        }
        Ok(changed)
    }

    /// 全部活跃任务失效（进程退出 / 恢复对账）；返回失效数量。
    pub(crate) fn invalidate_all(&self, reason: BackgroundInvalidationReason) -> usize {
        let mut tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let mut invalidated_count = 0;
        for task in tasks.values_mut() {
            if task.record.is_terminal() {
                continue;
            }
            let advanced = task
                .record
                .clone()
                .advance(BackgroundProcessState::Terminal(
                    BackgroundProcessTerminalKind::Invalidated { reason },
                ));
            if let Ok(BackgroundProcessAdvance {
                record,
                changed: true,
            }) = advanced
            {
                task.record = record;
                task.record.mark_finished(SystemTime::now());
                invalidated_count += 1;
            }
        }
        invalidated_count
    }

    /// 单任务记录快照。
    pub(crate) fn snapshot(
        &self,
        task_id: &BackgroundProcessId,
    ) -> Option<BackgroundProcessRecord> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        tasks.get(task_id).map(|task| task.record.clone())
    }

    /// 全部任务记录快照。
    pub(crate) fn snapshots(&self) -> Vec<BackgroundProcessRecord> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        tasks.values().map(|task| task.record.clone()).collect()
    }

    fn advance(
        &self,
        task_id: &BackgroundProcessId,
        next: BackgroundProcessState,
    ) -> Result<(), BackgroundProcessSupervisorError> {
        let mut tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let task = tasks.get_mut(task_id).ok_or_else(|| {
            BackgroundProcessSupervisorError::TaskNotFound(task_id.as_str().to_string())
        })?;
        let advanced = task.record.clone().advance(next)?;
        task.record = advanced.record;
        Ok(())
    }

    /// 取走「终态且未通知」的任务完成条目（take 语义，#252 通知链路）。
    ///
    /// - 每条完成事实只通知一次：**注入确认制**——`peek` 只读不标记，
    ///   `mark_notified` 由 reminder source 在快照被组装进 window 后
    ///   调用（#252：take 即标记在「有 active Run 但已无后续 step」时
    ///   把完成事实静默丢失，实测 sleep-20 通知从未到达任何 LLM 请求）。
    ///   未确认的事实保留在监督器，由下一个 Run / wakeup 补注入。
    /// - `Invalidated` 是生命周期失效（resume / terminate 场景走失效
    ///   投影），不产生 LLM 通知。
    pub(crate) fn peek_unnotified_terminal_items(
        &self,
    ) -> Vec<context::BackgroundProcessReminderItemData> {
        let tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let mut items = Vec::new();
        for task in tasks.values() {
            if task.notified {
                continue;
            }
            let Some(kind) = task.record.terminal_kind() else {
                continue;
            };
            let Some(status) = terminal_completion_status(&kind) else {
                continue;
            };
            let output_tail = match &task.record.log_file {
                Some(log_path) => {
                    let log = TaskLogFile::from_path(log_path.clone());
                    let size = log.size_bytes();
                    let start = size.saturating_sub(
                        crate::application::constants::BACKGROUND_PROCESS_NOTIFICATION_TAIL_BYTES
                            as u64,
                    );
                    log.read_range(start, usize::MAX)
                        .map(|segment| {
                            strip_internal_markers(&String::from_utf8_lossy(&segment.bytes))
                        })
                        .unwrap_or_default()
                }
                None => String::new(),
            };
            let output_tail = if output_tail.is_empty() {
                task.terminal_output.clone().unwrap_or_default()
            } else {
                output_tail
            };
            items.push(context::BackgroundProcessReminderItemData {
                task_id: task.record.task_id.as_str().to_string(),
                tool_name: task.record.identity.tool_name.clone(),
                status,
                output_tail,
            });
        }
        // 稳定顺序：按 task_id（UUIDv7 单调）排序，注入内容可复现。
        items.sort_by(|left, right| left.task_id.cmp(&right.task_id));
        items
    }

    /// 注入确认：把已组装进 window 的事实标记为已通知（幂等；未列出的
    /// 保持未标记，供后续 Run 补注入）。
    pub(crate) fn mark_notified(&self, task_ids: &[String]) {
        let mut tasks = self.tasks.lock().expect("后台进程表锁中毒");
        for task_id in task_ids {
            if let Ok(process_id) = BackgroundProcessId::parse(task_id) {
                if let Some(task) = tasks.get_mut(&process_id) {
                    task.notified = true;
                }
            }
        }
    }
}

/// 任务终态 → reminder 通知状态映射；`Invalidated` 返回 None（不通知）。
fn terminal_completion_status(
    kind: &BackgroundProcessTerminalKind,
) -> Option<context::BackgroundProcessCompletionStatus> {
    match kind {
        BackgroundProcessTerminalKind::Success => {
            Some(context::BackgroundProcessCompletionStatus::Succeeded)
        }
        BackgroundProcessTerminalKind::Failure => {
            Some(context::BackgroundProcessCompletionStatus::Failed)
        }
        BackgroundProcessTerminalKind::TimedOut => {
            Some(context::BackgroundProcessCompletionStatus::TimedOut)
        }
        BackgroundProcessTerminalKind::Stopped => {
            Some(context::BackgroundProcessCompletionStatus::Cancelled)
        }
        BackgroundProcessTerminalKind::Invalidated { .. } => None,
    }
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;

impl BackgroundProcessSupervisor {
    /// 从持久化快照恢复记录（#252 PR3 resume）：终态记录原样恢复；
    /// 非终态标 `Invalidated(ProcessExit)`（执行体随原进程消亡）。
    /// 返回恢复条数（同 id 幂等覆盖）。
    pub(crate) fn restore_records(&self, records: Vec<BackgroundProcessRecord>) -> usize {
        let mut tasks = self.tasks.lock().expect("后台进程表锁中毒");
        let mut restored = 0;
        for mut record in records {
            if !record.is_terminal() {
                if let Ok(advanced) = record.clone().advance(BackgroundProcessState::Terminal(
                    BackgroundProcessTerminalKind::Invalidated {
                        reason: BackgroundInvalidationReason::ProcessExit,
                    },
                )) {
                    let mut invalid_record = advanced.record;
                    invalid_record.mark_finished(SystemTime::now());
                    record = invalid_record;
                }
            }
            let task_id = record.task_id.clone();
            tasks.insert(
                task_id.clone(),
                SupervisedBackgroundProcess {
                    record,
                    terminal_output: None,
                    // 恢复记录不再进通知通道（resume 场景经失效投影展示）。
                    notified: true,
                    child_cancellation: tokio_util::sync::CancellationToken::new(),
                },
            );
            restored += 1;
        }
        restored
    }
}

/// 通知尾部视图剥离内部执行标记（#1890 直绑文件含 Bash 注入的
/// `__AEMEATH_CWD__=` marker 行与 `[cwd: ...]` 尾行——面向 LLM 的
/// 通知文本不得泄漏内部标记）。
fn strip_internal_markers(tail: &str) -> String {
    tail.lines()
        .filter(|line| {
            !line.contains("__AEMEATH_CWD__=")
                && !(line.starts_with("[cwd: ") && line.ends_with(']'))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
