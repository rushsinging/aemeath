use std::sync::Arc;

use super::TaskStore;
use crate::{TaskAccess, TaskPersist};

/// Composition root for the TaskData BC.
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

/// Wires a fresh, empty TaskData BC and returns its composition root.
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
#[path = "wiring_tests.rs"]
mod tests;
