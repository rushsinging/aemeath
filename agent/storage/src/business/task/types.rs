pub use share::task::{Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus};

pub(crate) fn default_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or_default()
}
