//! task 操作的公共接口
//!
//! tools 通过此模块调用 core 的 task 类型，
//! 避免直接引用 aemeath_core::task。

pub use aemeath_core::task::{
    BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStore,
};
