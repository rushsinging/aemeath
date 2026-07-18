use std::sync::Arc;

use super::{TaskAccess, TaskPersist, TaskStore};

/// Composition root for the Task BC.
///
/// `TaskWiring` owns the single [`TaskStore`] backing behind an [`Arc`] and
/// hands out only capability-typed, composition-only views of it. The backing
/// `Arc<TaskStore>` is a private field and is never returned, so consumers can
/// depend on [`TaskAccess`] or [`TaskPersist`] without ever naming the concrete
/// store or reaching its crate-private plumbing.
///
/// Every view shares the one backing: a command applied through
/// [`access`](Self::access) is observable through a snapshot collected via
/// [`persist`](Self::persist), and vice versa.
pub struct TaskWiring {
    store: Arc<TaskStore>,
}

/// Wires a fresh, empty Task BC and returns its composition root.
pub fn wire_task() -> TaskWiring {
    log::info!(target: crate::LOG_TARGET, "wire_task: enter");
    let wiring = TaskWiring {
        store: Arc::new(TaskStore::new()),
    };
    log::info!(target: crate::LOG_TARGET, "wire_task: ready");
    wiring
}

impl TaskWiring {
    /// A shared, composition-only [`TaskAccess`] view of the single backing.
    pub fn access(&self) -> Arc<dyn TaskAccess> {
        self.store.clone()
    }

    /// A shared, composition-only [`TaskPersist`] view of the single backing.
    pub fn persist(&self) -> Arc<dyn TaskPersist> {
        self.store.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::{
        BatchCreateSpec, TaskCreateSpec, TaskPriority, TaskSnapshot, TaskSnapshotValidationError,
    };

    fn batch_spec(name: &str) -> BatchCreateSpec {
        BatchCreateSpec::try_new(name.into()).unwrap()
    }

    fn task_spec(name: &str) -> TaskCreateSpec {
        TaskCreateSpec::try_new(name.into(), String::new(), None, TaskPriority::Normal).unwrap()
    }

    #[test]
    fn access_and_persist_views_share_the_one_backing() {
        let wiring = wire_task();
        let access = wiring.access();
        let persist = wiring.persist();

        // A command applied through the access view is visible through the
        // persist view's snapshot: both are the same backing store.
        let batch = access.create_batch(batch_spec("批次"), 1).unwrap().value;
        let created = access.create_task(task_spec("任务"), 2).unwrap().value;

        let snapshot = persist.collect_snapshot();
        assert_eq!(snapshot.current_batch(), Some(batch.id()));
        assert_eq!(snapshot.tasks().len(), 1);
        assert_eq!(snapshot.tasks()[0].id(), created.id());

        // A restore committed through the persist view is observable through the
        // access view, confirming the shared backing in the other direction.
        let empty = persist
            .prepare_restore(&TaskSnapshot::empty())
            .expect("empty snapshot restores");
        persist.commit_restore(empty);
        assert!(access.list().is_empty());
        assert!(access.list_batches().is_empty());
    }

    #[test]
    fn collect_prepare_commit_round_trips_empty_and_nonempty_through_the_port() {
        let wiring = wire_task();
        let persist = wiring.persist();
        let access = wiring.access();

        // Empty: collect -> prepare -> commit is a valid no-op restore.
        let empty = persist.collect_snapshot();
        assert!(empty.tasks().is_empty());
        let prepared = persist.prepare_restore(&empty).expect("empty restores");
        persist.commit_restore(prepared);
        assert!(access.list().is_empty());

        // Nonempty: a captured live image round-trips back to the same lists.
        access.create_batch(batch_spec("批次"), 1).unwrap();
        access.create_task(task_spec("任务"), 2).unwrap();
        let nonempty = persist.collect_snapshot();
        assert_eq!(nonempty.tasks().len(), 1);

        let target = wire_task();
        let target_access = target.access();
        let target_persist = target.persist();
        let prepared = target_persist
            .prepare_restore(&nonempty)
            .expect("captured live snapshot restores");
        target_persist.commit_restore(prepared);
        assert_eq!(target_access.list(), access.list());
        assert_eq!(target_access.list_batches(), access.list_batches());
    }

    #[test]
    fn prepare_restore_failure_leaves_live_backing_and_snapshot_untouched() {
        let wiring = wire_task();
        let access = wiring.access();
        let persist = wiring.persist();
        access.create_batch(batch_spec("批次"), 1).unwrap();
        access.create_task(task_spec("任务"), 2).unwrap();
        let before = persist.collect_snapshot();

        // A self-referential dependency cannot pass validation. `prepare_restore`
        // clones before validating, so both the argument snapshot and the live
        // backing survive the rejection unchanged.
        let bytes = br#"{"schema_version":2,"revision":"1","tasks":[{"id":"1","batch":"1","subject":"t","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":["1"],"status":"pending","priority":"normal","created_at":1,"updated_at":1,"started_at":null,"completed_at":null}],"next_task_id":"2","next_batch_id":"2","current_batch":"1","batches":[{"id":"1","summary":"b","status":"active","created_at":1,"last_active_turn":0,"silence_turns":0}]}"#;
        let invalid = TaskSnapshot::decode(bytes).expect("well-formed wire data");

        assert!(matches!(
            persist.prepare_restore(&invalid),
            Err(TaskSnapshotValidationError::SelfDependency { .. })
        ));
        // The argument snapshot is untouched (still decodable/equal) and live
        // state is byte-for-byte unchanged.
        assert_eq!(invalid, TaskSnapshot::decode(bytes).unwrap());
        assert_eq!(persist.collect_snapshot(), before);
    }

    // -----------------------------------------------------------------
    // Log-capture test infrastructure
    // -----------------------------------------------------------------

    thread_local! {
        static CAPTURED_LOGS: std::cell::RefCell<Vec<(String, log::Level, String)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if record.target() == crate::LOG_TARGET {
                CAPTURED_LOGS.with(|cell| {
                    cell.borrow_mut().push((
                        record.target().to_owned(),
                        record.level(),
                        format!("{}", record.args()),
                    ));
                });
            }
        }

        fn flush(&self) {}
    }

    /// Installs the capturing logger exactly once per test process. Safe to
    /// call from every test: `log::set_logger` only succeeds once, subsequent
    /// calls are no-ops via `Once`. Capture storage is thread-local, so tests
    /// on different OS threads never observe each other's records.
    fn install_capturing_logger() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_boxed_logger(Box::new(CapturingLogger))
                .expect("capturing logger must install exactly once per process");
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    /// Drains (and clears) whatever crate-targeted records this thread has
    /// captured so far.
    fn drain_captured_logs() -> Vec<(String, log::Level, String)> {
        CAPTURED_LOGS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
    }

    #[test]
    fn wire_task_logs_enter_and_ready_with_no_failure() {
        install_capturing_logger();
        drain_captured_logs();

        let _wiring = wire_task();

        let logs = drain_captured_logs();

        // Contract: the real low-frequency entry records that it was entered …
        let enter_pos = logs.iter().position(|(_, _, msg)| msg.contains("enter"));
        assert!(
            enter_pos.is_some(),
            "wire_task must log an enter marker; got {logs:?}"
        );
        // … and that it reached a successful exit (ready).
        let ready_pos = logs.iter().position(|(_, _, msg)| msg.contains("ready"));
        assert!(
            ready_pos.is_some(),
            "wire_task must log a ready (success-exit) marker; got {logs:?}"
        );
        // enter precedes ready.
        assert!(
            enter_pos.is_some_and(|e| ready_pos.is_some_and(|r| e < r)),
            "enter must precede ready; got {logs:?}"
        );
        // This entry has no failure path: it must not emit error/warn logs.
        assert!(
            logs.iter()
                .all(|(_, level, _)| !matches!(level, log::Level::Error | log::Level::Warn)),
            "wire_task must never emit failure-level logs; got {logs:?}"
        );
        // Every record carries the crate's LOG_TARGET.
        assert!(
            logs.iter()
                .all(|(target, _, _)| target == crate::LOG_TARGET),
            "all records must use crate LOG_TARGET; got {logs:?}"
        );
    }
}
