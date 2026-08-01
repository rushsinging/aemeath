use std::sync::Arc;

use task::TaskAccess;

pub(crate) fn test_task_access() -> Arc<dyn TaskAccess> {
    task::wire_task().access()
}
