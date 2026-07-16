//! Application state management — Settings 类型定义。
//!
//! Session 管理已迁移到 context crate（context::session）。
//! InternalSession / AppState / SessionMessage 已删除。

pub mod settings;
pub use settings::{PermissionMode, Settings};
