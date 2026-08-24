//! Session 聚合子模块（Session、ChatChain、ChatSegment）。
//!
//! 设计文档：`docs/design/02-modules/context-management/01-session.md`

mod chat_chain;
mod envelope;
mod generation;
mod management;
mod message_integrity;
mod restore;
mod types;

pub use chat_chain::{ChatChain, ChatSegment, SegmentKind};
pub use envelope::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, CommittedStep, CommittedStepLedger, CommittedStepMessages, DecodedSession,
    FinalizedOutcomeProjection, RunStepCursor, SessionCodec, SessionCodecError, SessionHistory,
    SkillLoadRecord, SnapshotState, CURRENT_SESSION_SCHEMA_VERSION,
};
pub use generation::{
    DisplayHistoryStepIndex, DisplayHistoryStepReference, DisplayHistoryStepWindow,
    SessionCommitPlan, SessionGenerationCodec, SessionGenerationManifest,
    SessionGenerationWireError, SessionMemberBytes, SessionMetadataMember, SessionStateMember,
    SessionStepMember, SessionStepReference,
};
pub use management::{
    same_project_identity, session_matches_project, SessionListEntry, SessionManagementError,
    SessionMetadataUpdate, SessionResumeLoad, SessionResumeView,
};
pub use restore::{SessionRestore, SessionRestoreStep};
pub use types::{
    extract_project_name, new_session_id, now_iso, validate_session_id, PersistedWorkspaceContext,
    PersistedWorkspaceFrame, SessionMetadata,
};
