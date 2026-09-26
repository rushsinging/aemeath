//! # TaskData BC 公开 façade（Issue #887 · TaskData #3）
//!
//! 正式 façade 只发布 [`TaskAccess`]、[`TaskStore`] 以及 typed
//! commands / results / entities / read models。聚合状态、其内部
//! map / counter、实体可变逃逸方法与仅测试装配构造器 **NEVER** 进入公开
//! API 面。下列 `compile_fail` 门禁把该边界钉进 `cargo test`：外部消费者
//! 编译不过即为期望行为。
//!
//! 正式 façade 保持可达（防止过度收窄）：
//! ```
//! use task::{BatchCreateSpecData, TaskAccess, TaskCreateSpecData, TaskPriorityData, TaskStore};
//! let store = TaskStore::new();
//! let access: &dyn TaskAccess = &store;
//! assert!(access.list().is_empty());
//! let batch = BatchCreateSpecData::try_new("batch".to_owned()).expect("valid summary");
//! access.create_batch(batch, 0).expect("create batch");
//! let spec = TaskCreateSpecData::try_new("t".to_owned(), String::new(), None, TaskPriorityData::Normal)
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
//! let _ = store.blocking_ids(task::TaskIdData::new(1));
//! ```
//!
//! 聚合内部状态 `TaskStoreState` 不是公开类型：
//! ```compile_fail
//! let _state = task::TaskStoreState::empty();
//! ```
//!
//! Snapshot validation is public. #890 publishes the persistence boundary as the
//! [`TaskPersist`] port plus the opaque [`PreparedTaskRestoreData`] token, wired
//! through [`TaskWiring`] / [`wire_task`]; the inherent capture / prepare /
//! install plumbing stays crate-private so consumers can only round-trip through
//! the port:
//! ```
//! use std::sync::Arc;
//! use task::{wire_task, TaskAccess, TaskPersist};
//! let wiring = wire_task();
//! let access: Arc<dyn TaskAccess> = wiring.access();
//! let persist: Arc<dyn TaskPersist> = wiring.persist();
//! let snapshot = persist.collect_snapshot();
//! let prepared = persist.prepare_restore(&snapshot).expect("empty snapshot restores");
//! persist.commit_restore(prepared);
//! assert!(access.list().is_empty());
//! ```
//! The crate-private plumbing behind the port stays unreachable:
//! ```compile_fail
//! let store = task::TaskStore::new();
//! let _ = store.capture_snapshot();
//! ```
//! ```compile_fail
//! let snapshot = task::TaskSnapshotData::empty();
//! let _ = snapshot.prepare();
//! ```
//! ```compile_fail
//! let store = task::TaskStore::new();
//! store.install_snapshot(());
//! ```
//! `PreparedTaskRestoreData` is public but opaque: no constructor, no field access,
//! no `Clone`, no serde. It cannot be built outside the crate:
//! ```compile_fail
//! let _ = task::PreparedTaskRestoreData { candidate: unreachable!() };
//! ```
//! Its wrapped state cannot be reached:
//! ```compile_fail
//! fn peek(prepared: task::PreparedTaskRestoreData) {
//!     let _ = prepared.candidate;
//! }
//! ```
//! A prepared token is single-use: `commit_restore` consumes it by value, so it
//! cannot be committed twice:
//! ```compile_fail
//! let wiring = task::wire_task();
//! let persist = wiring.persist();
//! let snapshot = persist.collect_snapshot();
//! let prepared = persist.prepare_restore(&snapshot).unwrap();
//! persist.commit_restore(prepared);
//! persist.commit_restore(prepared);
//! ```
//! `TaskWiring` hands out only capability views; the concrete backing never
//! escapes:
//! ```compile_fail
//! let wiring = task::wire_task();
//! let _backing: std::sync::Arc<task::TaskStore> = wiring.persist();
//! ```
//! `TaskStore` has no external stateful restore owner in #888:
//! ```compile_fail
//! let store = task::TaskStore::new();
//! store.restore_bytes(b"{}").unwrap();
//! ```
//!
//! 实体工厂构造器不对外发布（构造经 [`TaskAccess`] 意图命令）：
//! ```compile_fail
//! let _factory = task::TaskData::create;
//! ```
//!
//! 实体从不向外部持有者交出可变逃逸（`&mut TaskData` 字段写权限）：
//! ```compile_fail
//! fn escape(task: &mut task::TaskData) {
//!     task.set_priority(task::TaskPriorityData::High, 0);
//! }
//! ```
//! ```compile_fail
//! fn escape(task: &mut task::TaskData) {
//!     task.add_tag("x".to_owned(), 0);
//! }
//! ```
//! ```compile_fail
//! fn escape(batch: &mut task::BatchData) {
//!     let _ = batch.transition_to(task::BatchStatusData::Archived);
//! }
//! ```

pub(crate) const LOG_TARGET: &str = "aemeath:agent:task";
mod adapters;
mod domain;

pub use adapters::{wire_task, TaskStore, TaskWiring};
pub use domain::{
    BatchCreateSpecData, BatchData, BatchIdData, BatchStatusData, InterruptedBatchInfoData,
    PreparedTaskRestoreData, StaleBatchInfoData, TaskAccess, TaskBatchSnapshotData,
    TaskCommandResultData, TaskCreateSpecData, TaskData, TaskEventData, TaskIdData,
    TaskLifecycleSnapshotData, TaskPersist, TaskPriorityData, TaskPriorityStatsData,
    TaskProgressItemData, TaskProgressSnapshotData, TaskRevisionData, TaskSnapshotData,
    TaskStatusData, TaskStoreStatsData, TaskViewData,
};
