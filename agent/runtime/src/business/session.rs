//! 会话状态与持久层
//!
//! ## 模块结构
//! - `types` — 核心类型定义（Session, SessionMetadata, SessionFilter）
//! - `storage` — 序列化 / 反序列化 / 文件持久层
//! - `search` — 会话搜索与过滤

mod search;
mod storage;
mod types;

pub use search::search_sessions;
pub use storage::{
    delete_session, list_sessions, load_session, save_session, update_session_metadata,
};
pub use types::{
    extract_project_name, new_session_id, now_iso, sessions_dir, validate_session_id, Session,
    SessionFilter, SessionMetadata, WorkspaceContext, WorkspaceStackEntry,
};

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
