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
    TaskWiring {
        store: Arc::new(TaskStore::new()),
    }
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
}
