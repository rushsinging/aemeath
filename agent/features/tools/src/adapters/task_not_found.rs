//! Task not-found 错误文案（TaskUpdate / TaskBlockBy 共享）。
//!
//! 裸 "Task not found" 缺少列表上下文：当调用方误把 TaskListCreate 当
//! TaskCreate 连调后，当前列表为空，报错无法引导自纠。报错 MUST 携带
//! 当前列表状态（列表 id、任务数或 valid ids）与 TaskCreate 引导。

use crate::adapters::constants::TASK_LIST_NOT_TASK_HINT;
use task::{BatchIdData, TaskAccess};

/// 当前 active 列表内的 not-found 文案（task_list_id 未显式指定时）。
pub(crate) fn current_list_task_not_found_text(access: &dyn TaskAccess, value: &str) -> String {
    match access.current_batch() {
        None => format!(
            "Task not found in current task list: {value} \
(no active task list; create ONE list with TaskListCreate, \
then add tasks with TaskCreate)"
        ),
        Some(batch_id) => format!(
            "Task not found in current task list #{batch_id}: {value} ({})",
            snapshot_task_context(access, batch_id)
        ),
    }
}

/// 显式 task_list_id 指定列表内的 not-found 文案。
pub(crate) fn explicit_list_task_not_found_text(
    access: &dyn TaskAccess,
    batch_id: BatchIdData,
    value: &str,
) -> String {
    format!(
        "Task not found in task list #{batch_id}: {value} ({})",
        snapshot_task_context(access, batch_id)
    )
}

/// 列表快照上下文：列表缺失 / 空列表引导 / 非空列表列 valid ids。
fn snapshot_task_context(access: &dyn TaskAccess, batch_id: BatchIdData) -> String {
    match access.batch_snapshot(batch_id) {
        None => "task list not found; list ids come from TaskLists/TaskListGet".to_string(),
        Some(snapshot) => {
            let task_seqs: Vec<String> = snapshot
                .tasks()
                .iter()
                .map(|task| task.seq().to_string())
                .collect();
            if task_seqs.is_empty() {
                format!("list has 0 tasks; {TASK_LIST_NOT_TASK_HINT}")
            } else {
                format!("valid task ids: {}", task_seqs.join(", "))
            }
        }
    }
}

#[cfg(test)]
#[path = "task_not_found_tests.rs"]
mod tests;
