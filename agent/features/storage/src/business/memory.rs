//! Memory 持久化（fs IO）。
//!
//! Memory 的 DTO / 纯函数（MemoryEntry、枚举、error、scoring、dedup、format、
//! SessionReminders）留在 `share::memory` 作为共享内核；本模块只承载带文件系统
//! IO 的持久化职责（MemoryStore + 路径解析），归位 storage domain（047 spec §13）。

pub mod path;
pub mod store;

pub use path::{memory_base_dir, project_file_name, project_file_name_from_path};
pub use store::MemoryStore;
