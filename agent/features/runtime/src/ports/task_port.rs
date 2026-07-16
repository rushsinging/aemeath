//! TaskPort — Task BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #885 负责；此处复用已有 storage 类型。

use std::collections::HashMap;

use async_trait::async_trait;
use storage::{Task, TaskSnapshot};

// ─── Port trait ───

/// Task BC 的出站端口。
///
/// Sub Run 使用独立实例（`TaskStore::new()`），不共享父 Run 的 Task 状态。
///
/// 此 trait 收敛了 `core/port.rs` 中已有的 `TaskStorePort`，后续 #885 迁移时
/// 统一到此处，删除旧 `TaskStorePort`。
#[async_trait]
pub trait TaskPort: Send + Sync {
    /// 快照当前全部任务状态。
    async fn snapshot(&self) -> TaskSnapshot;

    /// 从快照恢复任务状态。
    async fn restore(&self, snapshot: TaskSnapshot);

    /// 列出当前批次的任务。
    async fn list_current(&self) -> Vec<Task>;

    /// 获取当前批次的显示映射（task name → display index）。
    async fn get_batch_display_map(&self) -> HashMap<String, usize>;
}
