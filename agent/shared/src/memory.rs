//! Memory 共享内核：DTO / 枚举 / error / 纯函数（scoring、dedup、format）与
//! SessionReminders。
//!
//! 旧 Memory 持久化业务 façade 已退役；当前持久化由 Memory BC 经
//! Storage 原子 dataset 机制完成，不再留在 share 共享内核。

pub mod dedup;
pub mod entry;
pub mod error;
pub mod format;
pub mod result;
pub mod scoring;
pub mod session_reminder;

pub use entry::{MemoryCategory, MemoryEntry, MemoryLayer, MemorySource};
pub use error::{MemoryError, MemoryResult};
pub use format::{format_add_result, format_memory_list, parse_category, parse_layer, short_id};
pub use result::{AddResult, CompactResult, MemoryStats};
pub use session_reminder::{SessionReminder, SessionReminders};
