//! Session 聚合子模块（Session、ChatChain、ChatSegment）。
//!
//! 设计文档：`docs/design/02-modules/context-management/01-session.md`

mod chat_chain;
mod envelope;
mod management;
mod message_integrity;
mod restore;
mod types;

pub use chat_chain::{ChatChain, ChatSegment, SegmentKind};
pub use envelope::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, CommittedStep, DecodedSession, FinalizedOutcomeProjection, RunStepCursor,
    SessionCodec, SessionCodecError, SnapshotState, CURRENT_SESSION_SCHEMA_VERSION,
};
pub use management::{
    SessionListEntry, SessionManagementError, SessionMetadataUpdate, SessionResumeProjection,
};
pub use restore::SessionRestore;
pub use types::{
    extract_project_name, new_session_id, now_iso, validate_session_id, PersistedWorkspaceContext,
    PersistedWorkspaceFrame, SessionMetadata,
};
