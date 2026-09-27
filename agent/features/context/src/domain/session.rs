//! Session 聚合子模块（Session、ChatChain、ChatSegment）。
//!
//! 设计文档：`docs/design/02-modules/context-management/01-session.md`

mod chat_chain;
mod envelope;
mod generation;
mod management;
mod message_integrity;
mod project_layout;
mod restore;
mod types;

pub(crate) use chat_chain::ChatSegment;
pub use chat_chain::{ChatChain, SegmentKind};
pub(crate) use envelope::{
    AcceptedInputRecord, ActiveCompactMarker, CommittedRunSlice, CommittedRunStep,
    FinalizedOutcomeRecord, RunStepCursor, SessionCodec, SessionCodecError, SessionHistory,
    CURRENT_SESSION_SCHEMA_VERSION,
};
pub use envelope::{
    CanonicalSession, CommittedStep, CommittedStepLedger, CommittedStepMessages, DecodedSession,
    SkillLoadRecord, SnapshotState,
};
pub use generation::{
    DisplayHistoryStepIndex, DisplayHistoryStepWindow, SessionMemberBytes, SessionMetadataMember,
    SessionStateMember, SessionStepMember, SessionStepReference,
};
pub(crate) use generation::{
    DisplayHistoryStepReference, SessionCommitPlan, SessionGenerationCodec,
    SessionGenerationManifest, SessionGenerationWireError,
};
pub use management::{
    same_project_identity, session_matches_project, SessionListEntry, SessionManagementError,
    SessionMetadataUpdate, SessionResumeLoad, SessionResumeView,
};
pub(crate) use project_layout::project_dir_segment;
pub use project_layout::session_project_dir;
pub use restore::{SessionRestore, SessionRestoreStep};
pub(crate) use types::PersistedWorkspaceFrame;
pub use types::{
    extract_project_name, new_session_id, now_iso, validate_session_id, PersistedWorkspaceContext,
    SessionMetadata,
};
