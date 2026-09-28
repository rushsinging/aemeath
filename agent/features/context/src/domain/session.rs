//! Session 聚合子模块（Session、ChatChain、ChatSegment）。
//!
//! 设计文档：`docs/design/02-modules/context-management/01-session.md`

pub(crate) use generation::{
    SessionCommitPlan, SessionGenerationCodec, SessionGenerationManifest,
    SessionGenerationWireError,
};

pub use share::session_types::PersistedWorkspaceContext;

mod chat_chain;
mod envelope;
mod generation;
mod management;
mod message_integrity;
mod project_layout;
mod restore;
mod types;

pub(crate) use chat_chain::ChatSegment;
pub use chat_chain::SegmentKind;
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
    DisplayHistoryStepIndexData, DisplayHistoryStepWindowData, SessionStepMember,
    SessionStepReference,
};
#[cfg(test)]
// workspace feature 统一下 dev 门控测试不参与 clippy 编译，显式标注未用豁免。
#[allow(unused_imports)]
pub use generation::{SessionMemberBytes, SessionMetadataMember, SessionStateMember};
pub use management::{
    same_project_identity, session_matches_project, SessionListEntryData, SessionManagementError,
    SessionMetadataUpdateData, SessionResumeLoad, SessionResumeViewData,
};
pub(crate) use project_layout::project_dir_segment;
pub use project_layout::session_project_dir;
pub use restore::{SessionRestore, SessionRestoreStepData};
pub use types::{extract_project_name, new_session_id, now_iso, SessionMetadata};
