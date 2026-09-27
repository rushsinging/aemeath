pub(crate) const LOG_TARGET: &str = "aemeath:context";

/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
mod adapters;
mod application;
mod domain;
mod ports;

pub(crate) use adapters::isolated_context_with_skill;
pub use adapters::{isolated_context, isolated_context_with_workspace_skills};
pub use adapters::{NoOpCanonicalSessionWriter, ProductionMainContextFactory};
pub(crate) use domain::session::DisplayHistoryStepReference;
pub use domain::session::{
    DisplayHistoryStepIndex, DisplayHistoryStepWindow, SessionListEntry, SessionManagementError,
    SessionMetadataUpdate, SessionRestoreStep, SessionResumeView,
};
pub use ports::SessionManagementPort;

// Main Session coordinator — cross-BC wiring for Runtime bootstrap.
#[cfg(any(test, feature = "dev"))]
pub use application::test_support;
pub use application::{
    wire_main_session, MainSessionDependencies, MainSessionWiring, MainSessionWiringBuilder,
    OwnedSessionSharedPermit,
};
pub(crate) use application::{BoundMainRun, MainSessionError, SessionSwitchGate};

// 窄 façade根导出（#1022 收口：内部层 mod 私有，跨 BC 消费只经 crate 根与语义模块）
#[cfg(any(test, feature = "dev"))]
pub(crate) use adapters::capture_session_lifecycle;
pub(crate) use adapters::{
    decode_session, skill_prompt_budget, AcceptedInputWriter, AtomicBlobCanonicalSessionWriter,
    AtomicBlobSessionStore, CanonicalSessionRepository, CanonicalSessionWriter,
    CommittedMemoryRetrieveAdapter, DatasetSessionReader, InMemorySessionRepository,
    LegacySessionDecoder, NoOpContextMemorySource, SkillPromptSource, ToolReceiptWriter,
};
pub use adapters::{
    AtomicBlobAcceptedInputWriter, AtomicBlobSessionManagement, AtomicBlobToolReceiptWriter,
    DatasetCanonicalSessionWriter, DatasetSessionManagement, WorkspaceSkillQueryFactory,
};
pub use application::main_session;
pub(crate) use application::ContextApplicationService;
pub(crate) use application::{SessionLoadError, SessionPersistenceService};
pub(crate) use domain::session::{
    project_dir_segment, AcceptedInputRecord, ActiveCompactMarker, ChatSegment, CommittedRunSlice,
    CommittedRunStep, FinalizedOutcomeRecord, RunStepCursor, SessionCodec, SessionCodecError,
    SessionCommitPlan, SessionGenerationCodec, SessionGenerationManifest,
    SessionGenerationWireError, SessionHistory, CURRENT_SESSION_SCHEMA_VERSION,
};
pub use domain::session::{CanonicalSession, CommittedStep, CommittedStepMessages, SnapshotState};
pub(crate) use domain::ContextMessages;
pub use domain::{
    AcceptedInputAppend, AcceptedInputError, ContextAppend, ContextRequestId, SessionRevision,
    SystemBlock, Urgency,
};
pub use domain::{
    AppendReceipt, CleanupConfirmation, CompactGenerationFailure, CompactGenerationFailureKind,
    CompactGenerationOutput, CompactOutcome, CompactRequest, CompactResult, CompactSkipReason,
    CompactSummaryQuality, CompactTrigger, CompactionDecision, ContentFingerprint,
    ContextAppendError, ContextPortError, ContextRequest, ContextWindow, DecisionReason,
    FinalizeCause, InvocationReminder, Language, ManualCompactRequest, RunStepId, SessionId,
    StepReceipt, SystemPromptSpec, TaskProgressReminder, TaskProgressReminderItem,
    TaskProgressStatus, ToolCallIdentity, ToolOutcomeKind, ToolReceiptMutation,
    ToolReceiptMutationError, ToolReceiptMutationReceipt, ToolTerminalReceipt,
};
pub(crate) use domain::{ToolCallReceipt, ToolCallState};
pub(crate) use ports::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    PromptMaterializationError, SessionGeneration, SessionRepository, SessionSnapshot,
    SessionSnapshotStore, SessionStoreError, SkillQueryFactory,
};
pub use ports::{ContextPort, MainContextFactory};

pub mod api {
    pub use crate::adapters::migrate_flat_sessions_to_project_dirs;
    pub(crate) use crate::adapters::MemoryRetrieveAdapter;
    pub(crate) use crate::domain::session::DisplayHistoryStepReference;
    pub use crate::domain::session::{DisplayHistoryStepIndex, DisplayHistoryStepWindow};
    pub(crate) use crate::ports::MemoryMaterialization;
}

pub mod context_port {
    pub use crate::domain::*;
    pub use crate::ports::ContextPort;
}

pub mod compact {
    pub use crate::adapters::compact_summary::*;
    pub use crate::domain::compact::*;
    pub(crate) use crate::domain::{
        autocompact_threshold, effective_context_window, estimate_message_tokens, estimate_tokens,
    };
    pub use crate::domain::{estimate_messages_tokens, estimate_tool_schemas_tokens};
}

pub mod guidance {
    pub use crate::adapters::prompt::{
        assess_guidance, init_guidance_dir, resolve_guidance_async, InstructionsLoadedHook,
    };
    pub(crate) use crate::adapters::prompt::{
        resolve_guidance, universal_execution_discipline, GuidanceAssessment,
    };
}

pub mod session {
    pub(crate) use crate::domain::session::PersistedWorkspaceFrame;
    pub use crate::domain::session::{
        CanonicalSession, PersistedWorkspaceContext, SessionMetadata, SnapshotState,
    };
}

#[cfg(test)]
mod integration_tests {
    mod application_service_contract;
    mod application_service_failures;
    mod canonical_session_repository;
    mod context_port_contract;
    mod dataset_session_reader;
    mod dataset_session_writer;
    mod guidance_contract;
    mod in_memory_session_backing;
    mod isolated_context_with_skill;
    mod main_session_config_facade;
    mod main_session_gate;
    mod main_session_wiring;
    mod session_envelope_codec;
    mod session_management_port_contract;
    mod session_persistence_service;
    mod session_recovery_scenarios;
    mod session_snapshot_store_contract;
    mod skill_prompt_pipeline;
    mod structured_context_candidate;
    mod tool_receipt_persistence_contract;
}
