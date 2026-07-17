//! # Task BC 公开 façade（Issue #887 · Task #3）
//!
//! 正式 façade 只发布 [`TaskAccess`]、[`TaskStore`] 以及 typed
//! commands / results / entities / read models。聚合状态、其内部
//! map / counter、实体可变逃逸方法与仅测试装配构造器 **NEVER** 进入公开
//! API 面。下列 `compile_fail` 门禁把该边界钉进 `cargo test`：外部消费者
//! 编译不过即为期望行为。
//!
//! 正式 façade 保持可达（防止过度收窄）：
//! ```
//! use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority, TaskStore};
//! let store = TaskStore::new();
//! let access: &dyn TaskAccess = &store;
//! assert!(access.list().is_empty());
//! let batch = BatchCreateSpec::try_new("batch".to_owned()).expect("valid summary");
//! access.create_batch(batch, 0).expect("create batch");
//! let spec = TaskCreateSpec::try_new("t".to_owned(), String::new(), None, TaskPriority::Normal)
//!     .expect("valid spec");
//! let created = access.create_task(spec, 1).expect("create task");
//! assert_eq!(access.get(created.value.id()), Some(created.value));
//! ```
//!
//! `TaskStore` 只作为 composition root 可构造、可注入的 backing 类型公开；外部调用
//! 必须经 [`TaskAccess`]，不能绕过端口调用其固有命令/查询方法：
//! ```compile_fail
//! let store = task::TaskStore::new();
//! let _ = store.revision();
//! ```
//! ```compile_fail
//! let _command = task::TaskStore::create_batch;
//! ```
//! ```compile_fail
//! let store = task::TaskStore::new();
//! let _ = store.current_batch();
//! ```
//! ```compile_fail
//! let store = task::TaskStore::new();
//! let _ = store.blocking_ids(task::TaskId::new(1));
//! ```
//!
//! 聚合内部状态 `TaskStoreState` 不是公开类型：
//! ```compile_fail
//! let _state = task::TaskStoreState::empty();
//! ```
//!
//! Snapshot validation is public, but capture, candidate preparation, and
//! installation remain crate-private until #890 publishes a persistence port:
//! ```compile_fail
//! let store = task::TaskStore::new();
//! let _ = store.capture_snapshot();
//! ```
//! ```compile_fail
//! let snapshot = task::TaskSnapshot::empty();
//! let _ = snapshot.prepare();
//! ```
//! ```compile_fail
//! let _ = task::PreparedTaskRestore;
//! ```
//! ```compile_fail
//! let store = task::TaskStore::new();
//! store.install_snapshot(());
//! ```
//! `TaskStore` has no external stateful restore owner in #888:
//! ```compile_fail
//! let store = task::TaskStore::new();
//! store.restore_bytes(b"{}").unwrap();
//! ```
//!
//! 实体工厂构造器不对外发布（构造经 [`TaskAccess`] 意图命令）：
//! ```compile_fail
//! let _factory = task::Task::create;
//! ```
//!
//! 实体从不向外部持有者交出可变逃逸（`&mut Task` 字段写权限）：
//! ```compile_fail
//! fn escape(task: &mut task::Task) {
//!     task.set_priority(task::TaskPriority::High, 0);
//! }
//! ```
//! ```compile_fail
//! fn escape(task: &mut task::Task) {
//!     task.add_tag("x".to_owned(), 0);
//! }
//! ```
//! ```compile_fail
//! fn escape(batch: &mut task::Batch) {
//!     let _ = batch.transition_to(task::BatchStatus::Archived);
//! }
//! ```

mod business;
mod core;

pub use business::{
    detect_batch_all_completed, detect_interrupted_batch, detect_stale_batches, Batch,
    BatchCreateSpec, BatchId, BatchStatus, InterruptedBatchInfo, StaleBatchInfo, Task,
    TaskCommandError, TaskCommandResult, TaskCreateSpec, TaskEvent, TaskId, TaskLifecycleSnapshot,
    TaskPriority, TaskPriorityStats, TaskReminderItem, TaskReminderSnapshot, TaskRevision,
    TaskSnapshot, TaskSnapshotCodecError, TaskSnapshotValidationError, TaskStatus, TaskStoreStats,
};
pub use core::{TaskAccess, TaskStore};
