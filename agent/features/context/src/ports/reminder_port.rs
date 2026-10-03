//! Runtime → Context 的 reminder 控制面（07-reminder-pipeline.md）。
//!
//! 管线归 Context、事实来源归 Runtime：Run 生命周期句柄与 typed 事件
//! 推送经本 port 进入；注入决策由 build_window 内部按 policy 自动完成，
//! Runtime 不感知 placement / dedup / 预算细节。

use std::sync::Arc;

use crate::domain::reminder::{ReminderEventSource, ReminderSource};
use crate::domain::RunId;

/// Reminder 管线控制 port：Run 启动创建句柄、结束销毁、事件推送。
pub trait ReminderControlPort: Send + Sync {
    /// Run 启动：为该 Run 创建 reminder 管线（同 RunId 重复创建替换旧管线）。
    fn create_reminder_pipeline(&self, run_id: RunId, sources: Vec<Arc<dyn ReminderSource>>);

    /// Run 结束：销毁该 Run 的 reminder 管线。
    fn drop_reminder_pipeline(&self, run_id: &RunId);

    /// Run 启动事件：`OnRunStart` 类 source 入队。
    fn reminder_run_started(&self, run_id: &RunId);

    /// 业务事件推送：`OnEvent(source)` 匹配的 source 入队。
    fn reminder_handle_event(&self, run_id: &RunId, event_source: &ReminderEventSource);

    /// task store 变更事件：`OnTaskMutation` 类 source 入队。
    fn reminder_task_mutated(&self, run_id: &RunId);

    /// step 边界事件：`OnStepInterval(n)` 在步数为 n 的倍数时重建入队。
    fn reminder_step_advanced(&self, run_id: &RunId, step: u64);
}
