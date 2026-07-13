//! Session 聚合子模块（Session、ChatChain、ChatSegment）。
//!
//! 设计文档：`docs/design/02-modules/context-management/01-session.md`

mod chat_chain;
mod message_integrity;
mod restore;
mod search;
mod storage;
mod types;

pub use chat_chain::{ChatChain, ChatSegment, SegmentKind};
pub use restore::SessionRestore;
pub use search::search_sessions;
pub use storage::{
    delete_session, list_sessions, load_session, save_session, update_session_metadata,
    SessionLoadError,
};
pub use types::{
    extract_project_name, new_session_id, now_iso, sessions_dir, validate_session_id,
    PersistedWorkspaceContext, PersistedWorkspaceFrame, Session, SessionFilter, SessionMetadata,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
