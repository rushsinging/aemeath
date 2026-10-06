//! 后台任务监督器：任务登记、状态推进、输出采集与失效收口。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use context::ToolCallIdentityData;
use share::ids::BackgroundTaskId;

use crate::application::constants::BACKGROUND_TASK_OUTPUT_CAPACITY_BYTES;
use crate::domain::background_task::{
    BackgroundInvalidationReason, BackgroundTaskAdvance, BackgroundTaskRecord, BackgroundTaskState,
    BackgroundTaskTerminalKind, BackgroundTaskTransitionError,
};
use crate::domain::output_ring_buffer::OutputRingBuffer;

/// 监督中的后台任务：领域记录 + 输出缓冲 + 终态结果。
///
/// 输出来源两档：progress 通道增量进 ring buffer；无增量的工具只有
/// 终态完整结果。读取时 ring buffer 优先，为空回退终态文本。
struct SupervisedBackgroundTask {
    record: BackgroundTaskRecord,
    output: OutputRingBuffer,
    terminal_output: Option<String>,
    /// 完成事实是否已进入通知通道（take_unnotified 语义）。
    notified: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BackgroundTaskSupervisorError {
    #[error("后台任务不存在：{0}")]
    TaskNotFound(String),
    #[error("后台任务状态推进失败：{0}")]
    Transition(#[from] BackgroundTaskTransitionError),
}

/// session 级后台任务监督器。
///
/// 所有方法短临界区同步访问；异步驱动（JoinHandle 托管、完成回调）
/// 由执行流改造层挂接，本类型只维护任务事实与查询视图。
pub(crate) struct BackgroundTaskSupervisor {
    tasks: Mutex<HashMap<BackgroundTaskId, SupervisedBackgroundTask>>,
}

impl Default for BackgroundTaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundTaskSupervisor {
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
    ) -> BackgroundTaskId {
        let task = SupervisedBackgroundTask {
            record: BackgroundTaskRecord::dispatch(identity, invocation_summary),
            output: OutputRingBuffer::new(BACKGROUND_TASK_OUTPUT_CAPACITY_BYTES),
            terminal_output: None,
            notified: false,
        };
        let task_id = task.record.task_id.clone();
        self.tasks
            .lock()
            .expect("后台任务表锁中毒")
            .insert(task_id.clone(), task);
        task_id
    }

    /// 转后台：推进任务状态并固化 deadline 快照。
    pub(crate) fn mark_backgrounded(
        &self,
        task_id: &BackgroundTaskId,
        deadline_snapshot: Option<SystemTime>,
    ) -> Result<(), BackgroundTaskSupervisorError> {
        self.advance(
            task_id,
            BackgroundTaskState::Backgrounded {
                backgrounded_at: SystemTime::now(),
                deadline_snapshot,
            },
        )
    }

    /// 终态推进：返回是否发生变化（重复终态幂等返回 false）。
    pub(crate) fn finish(
        &self,
        task_id: &BackgroundTaskId,
        kind: BackgroundTaskTerminalKind,
        terminal_output: Option<String>,
    ) -> Result<bool, BackgroundTaskSupervisorError> {
        let mut tasks = self.tasks.lock().expect("后台任务表锁中毒");
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| BackgroundTaskSupervisorError::TaskNotFound(task_id.as_str().into()))?;
        if terminal_output.is_some() {
            task.terminal_output = terminal_output;
        }
        let BackgroundTaskAdvance { record, changed } = task
            .record
            .clone()
            .advance(BackgroundTaskState::Terminal(kind))?;
        task.record = record;
        Ok(changed)
    }

    /// 全部活跃任务失效（进程退出 / 恢复对账）；返回失效数量。
    pub(crate) fn invalidate_all(&self, reason: BackgroundInvalidationReason) -> usize {
        let mut tasks = self.tasks.lock().expect("后台任务表锁中毒");
        let mut invalidated_count = 0;
        for task in tasks.values_mut() {
            if task.record.is_terminal() {
                continue;
            }
            let advanced = task.record.clone().advance(BackgroundTaskState::Terminal(
                BackgroundTaskTerminalKind::Invalidated { reason },
            ));
            if let Ok(BackgroundTaskAdvance {
                record,
                changed: true,
            }) = advanced
            {
                task.record = record;
                invalidated_count += 1;
            }
        }
        invalidated_count
    }

    /// 采集任务输出增量（progress 通道或终态完整输出）。
    pub(crate) fn record_output(&self, task_id: &BackgroundTaskId, chunk: &[u8]) {
        let mut tasks = self.tasks.lock().expect("后台任务表锁中毒");
        if let Some(task) = tasks.get_mut(task_id) {
            task.output.append(chunk);
        }
    }

    /// 非消耗性输出尾部读取（多次读取幂等）：ring buffer 优先，
    /// 无增量采集时回退终态完整结果。
    pub(crate) fn read_output_tail(
        &self,
        task_id: &BackgroundTaskId,
        max_bytes: usize,
    ) -> Option<(String, u64)> {
        let tasks = self.tasks.lock().expect("后台任务表锁中毒");
        tasks.get(task_id).map(|task| {
            if task.output.is_empty() {
                let terminal_length = task
                    .terminal_output
                    .as_ref()
                    .map(|output| output.len())
                    .unwrap_or(0);
                (
                    task.terminal_output.clone().unwrap_or_default(),
                    terminal_length as u64,
                )
            } else {
                task.output.read_tail_text(max_bytes)
            }
        })
    }

    /// 单任务记录快照。
    pub(crate) fn snapshot(&self, task_id: &BackgroundTaskId) -> Option<BackgroundTaskRecord> {
        let tasks = self.tasks.lock().expect("后台任务表锁中毒");
        tasks.get(task_id).map(|task| task.record.clone())
    }

    /// 全部任务记录快照。
    pub(crate) fn snapshots(&self) -> Vec<BackgroundTaskRecord> {
        let tasks = self.tasks.lock().expect("后台任务表锁中毒");
        tasks.values().map(|task| task.record.clone()).collect()
    }

    fn advance(
        &self,
        task_id: &BackgroundTaskId,
        next: BackgroundTaskState,
    ) -> Result<(), BackgroundTaskSupervisorError> {
        let mut tasks = self.tasks.lock().expect("后台任务表锁中毒");
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| BackgroundTaskSupervisorError::TaskNotFound(task_id.as_str().into()))?;
        let advanced = task.record.clone().advance(next)?;
        task.record = advanced.record;
        Ok(())
    }

    /// 取走「终态且未通知」的任务完成条目（take 语义，#252 通知链路）。
    ///
    /// - 每条完成事实只通知一次：注入确认前由 reminder 队列持有，
    ///   取走即视为已进入通知通道；注入前 Run 被取消的极端窗口由
    ///   background_tasks 查询工具兜底（设计 §12 风险表）。
    /// - `Invalidated` 是生命周期失效（resume / terminate 场景走失效
    ///   投影），不产生 LLM 通知。
    pub(crate) fn take_unnotified_terminal_items(
        &self,
    ) -> Vec<context::BackgroundTaskReminderItemData> {
        let mut tasks = self.tasks.lock().expect("后台任务表锁中毒");
        let mut items = Vec::new();
        for task in tasks.values_mut() {
            if task.notified {
                continue;
            }
            let Some(kind) = task.record.terminal_kind() else {
                continue;
            };
            let Some(status) = terminal_completion_status(&kind) else {
                task.notified = true;
                continue;
            };
            let (output_tail, _) = task.output.read_tail_text(
                crate::application::constants::BACKGROUND_TASK_NOTIFICATION_TAIL_BYTES,
            );
            let output_tail = if output_tail.is_empty() {
                task.terminal_output.clone().unwrap_or_default()
            } else {
                output_tail
            };
            task.notified = true;
            items.push(context::BackgroundTaskReminderItemData {
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
}

/// 任务终态 → reminder 通知状态映射；`Invalidated` 返回 None（不通知）。
fn terminal_completion_status(
    kind: &BackgroundTaskTerminalKind,
) -> Option<context::BackgroundTaskCompletionStatus> {
    match kind {
        BackgroundTaskTerminalKind::Success => {
            Some(context::BackgroundTaskCompletionStatus::Succeeded)
        }
        BackgroundTaskTerminalKind::Failure => {
            Some(context::BackgroundTaskCompletionStatus::Failed)
        }
        BackgroundTaskTerminalKind::TimedOut => {
            Some(context::BackgroundTaskCompletionStatus::TimedOut)
        }
        BackgroundTaskTerminalKind::Stopped => {
            Some(context::BackgroundTaskCompletionStatus::Cancelled)
        }
        BackgroundTaskTerminalKind::Invalidated { .. } => None,
    }
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;
