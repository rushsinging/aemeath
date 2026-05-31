#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageApiMarker;

pub use crate::business::task::{
    Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStoreStats,
};
