//! Session 聚合子模块（Session、ChatChain、ChatSegment）。
//!
//! 设计文档：`docs/design/02-modules/context-management/01-session.md`

mod chat_chain;
mod message_integrity;
mod restore;
mod types;

pub use chat_chain::{ChatChain, ChatSegment, SegmentKind};
pub use restore::SessionRestore;
pub use types::{
    extract_project_name, new_session_id, now_iso, validate_session_id, PersistedWorkspaceContext,
    PersistedWorkspaceFrame, Session, SessionFilter, SessionMetadata,
};

#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;
