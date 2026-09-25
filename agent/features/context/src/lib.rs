pub(crate) const LOG_TARGET: &str = "aemeath:context";

/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
mod adapters;
mod application;
mod domain;
mod ports;

pub use adapters::{
    isolated_context, isolated_context_with_skill, isolated_context_with_workspace_skills,
};
pub use adapters::{NoOpCanonicalSessionWriter, ProductionMainContextFactory};
pub use domain::session::{
    DisplayHistoryStepIndex, DisplayHistoryStepReference, DisplayHistoryStepWindow,
    SessionListEntry, SessionManagementError, SessionMetadataUpdate, SessionRestoreStep,
    SessionResumeView,
};
pub use ports::SessionManagementPort;

// Main Session coordinator — cross-BC wiring for Runtime bootstrap.
#[cfg(any(test, feature = "dev"))]
pub use application::test_support;
pub use application::{
    wire_main_session, BoundMainRun, MainSessionDependencies, MainSessionError, MainSessionWiring,
    MainSessionWiringBuilder, OwnedSessionSharedPermit, SessionSwitchGate,
};

// 窄 façade根导出（#1022 收口：内部层 mod 私有，跨 BC 消费只经 crate 根与语义模块）
#[cfg(any(test, feature = "dev"))]
pub use adapters::capture_session_lifecycle;
pub use adapters::{
    decode_session, skill_prompt_budget, AcceptedInputWriter, AtomicBlobAcceptedInputWriter,
    AtomicBlobCanonicalSessionWriter, AtomicBlobSessionManagement, AtomicBlobSessionStore,
    AtomicBlobToolReceiptWriter, CanonicalSessionRepository, CanonicalSessionWriter,
    CommittedMemoryRetrieveAdapter, DatasetCanonicalSessionWriter, DatasetSessionManagement,
    DatasetSessionReader, InMemorySessionRepository, LegacySessionDecoder, NoOpContextMemorySource,
    SkillPromptSource, ToolReceiptWriter, WorkspaceSkillQueryFactory,
};
pub use application::{main_session, ContextApplicationService};
pub use application::{SessionLoadError, SessionPersistenceService};
pub use domain::session::{
    project_dir_segment, AcceptedInputProjection, ActiveCompactMarker, CanonicalSession,
    ChatSegment, CommittedRunSlice, CommittedRunStep, CommittedStep, CommittedStepMessages,
    FinalizedOutcomeProjection, RunStepCursor, SessionCodec, SessionCodecError, SessionCommitPlan,
    SessionGenerationCodec, SessionGenerationManifest, SessionGenerationWireError, SessionHistory,
    SnapshotState, CURRENT_SESSION_SCHEMA_VERSION,
};
pub use domain::{
    AcceptedInputAppend, AcceptedInputError, ContextAppend, ContextMessages, ContextRequestId,
    SessionRevision, SystemBlock, Urgency,
};
pub use domain::{
    AppendReceipt, CleanupConfirmation, CompactGenerationFailure, CompactGenerationFailureKind,
    CompactGenerationOutput, CompactOutcome, CompactRequest, CompactResult, CompactSkipReason,
    CompactSummaryQuality, CompactTrigger, CompactionDecision, ContentFingerprint,
    ContextAppendError, ContextPortError, ContextRequest, ContextWindow, DecisionReason,
    FinalizeCause, InvocationReminder, Language, ManualCompactRequest, RunStepId, SessionId,
    StepReceipt, SystemPromptSpec, TaskProgressReminder, TaskProgressReminderItem,
    TaskProgressStatus, ToolCallIdentity, ToolCallReceipt, ToolCallState, ToolOutcomeKind,
    ToolReceiptMutation, ToolReceiptMutationError, ToolReceiptMutationReceipt, ToolTerminalReceipt,
};
pub use ports::{
    ContextMemorySource, ContextPort, ContextPromptSource, MainContextFactory,
    MemoryMaterialization, PromptMaterialization, PromptMaterializationError, SessionGeneration,
    SessionRepository, SessionSnapshot, SessionSnapshotStore, SessionStoreError, SkillQueryFactory,
};

pub mod api {
    pub use crate::adapters::migrate_flat_sessions_to_project_dirs;
    pub use crate::adapters::MemoryRetrieveAdapter;
    pub use crate::domain::session::{
        DisplayHistoryStepIndex, DisplayHistoryStepReference, DisplayHistoryStepWindow,
    };
    pub use crate::ports::MemoryMaterialization;
}

pub mod context_port {
    pub use crate::domain::*;
    pub use crate::ports::ContextPort;
}

pub mod compact {
    pub use crate::adapters::compact_summary::*;
    pub use crate::domain::compact::*;
    pub use crate::domain::{
        autocompact_threshold, effective_context_window, estimate_message_tokens,
        estimate_messages_tokens, estimate_tokens, estimate_tool_schemas_tokens,
    };
}

pub mod guidance {
    pub use crate::adapters::prompt::{
        assess_guidance, init_guidance_dir, resolve_guidance, resolve_guidance_async,
        universal_execution_discipline, GuidanceAssessment, InstructionsLoadedHook,
    };
}

pub mod session {
    pub use crate::domain::session::{
        CanonicalSession, PersistedWorkspaceContext, PersistedWorkspaceFrame, SessionMetadata,
        SnapshotState,
    };
}
