//! Re-exports task types from `tool::types::task` for backward compatibility.
//!
//! The canonical definitions live in `crate::tool::types::task` so that
//! `build.rs` can generate precise JSON Schema for them.

pub use crate::tool::types::task::{
    Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskTimestamps,
};
