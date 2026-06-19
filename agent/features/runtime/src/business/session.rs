//! 会话状态与持久层
//!
//! ## 模块结构
//! - `types` — 核心类型定义（Session, SessionMetadata, SessionFilter）
//! - `chat_chain` — Chat 链结构（ChatSegment, SegmentKind, ChatChain）
//! - `storage` — 序列化 / 反序列化 / 文件持久层
//! - `search` — 会话搜索与过滤

mod chat_chain;
mod search;
mod storage;
mod types;

pub use chat_chain::{ChatChain, ChatSegment, SegmentKind};
pub use search::search_sessions;
pub use storage::{
    delete_session, list_sessions, load_session, save_session, update_session_metadata,
};
pub use types::{
    extract_project_name, new_session_id, now_iso, sessions_dir, validate_session_id,
    PersistedWorkspaceContext, PersistedWorkspaceFrame, Session, SessionFilter, SessionMetadata,
};

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
