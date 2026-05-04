//! 会话状态与持久层
//!
//! ## 模块结构
//! - `types` — 核心类型定义（Session, SessionMetadata, SessionFilter）
//! - `storage` — 序列化 / 反序列化 / 文件持久层
//! - `search` — 会话搜索与过滤

mod types;
mod storage;
mod search;

pub use types::{extract_project_name, new_session_id, now_iso, sessions_dir, validate_session_id, Session, SessionFilter, SessionMetadata};
pub use storage::{delete_session, list_sessions, load_session, save_session, update_session_metadata};
pub use search::search_sessions;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
