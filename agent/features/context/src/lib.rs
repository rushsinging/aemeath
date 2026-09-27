//! Context：会话上下文的编排、持久化与压缩决策。
//!
//! # Published Language（四类语法，#1707 收敛·两阶段）
//!
//! | 组 | 实体 |
//! |---|---|
//! | 工厂 | `wire_main_session`（出口 `SdkError`）、`wire_isolated_context{,_with_skill,_with_workspace_skills}`（isolated_context 家族更名） |
//! | 角色和职能 | `ContextPort`（+context_port 模块 glob 面）、`SessionManagementPort`、`MainSessionWiring`（生产协调器）+ Gate 家族、适配器实现体（composition 装配面） |
//! | 数据和生命周期 | 36 个 Data（ContextRequest/StepReceipt/Compact*/ToolReceipt*/Session 展示类）；session 持久化内核（SessionCodec/Generation 族等）已内部化 pub(crate) |
//! | Error | SessionManagementError/ContextPortError/ContextAppendError/AcceptedInputError（活跃 match 保留）；MainSessionError 内部化（wire 出口统一 SdkError） |
//!
//! 实测修正（调研"零消费 65"口径错误——真死仅 2）：52 符号内部化后经
//! 编译器/测试暴露真消费 16 处恢复 pub（glob 链 11 + 签名载荷 5）；
//! 集成测试 20 文件（9782 行）搬 src/integration_tests/。
//! 按 docs/design/03-engineering/05-published-language.md SOP。

pub(crate) const LOG_TARGET: &str = "aemeath:context";

/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
mod adapters;
mod application;
mod domain;
mod ports;

#[cfg(any(test, feature = "dev"))]
#[cfg(any(test, feature = "dev"))]
pub(crate) use adapters::capture_session_lifecycle;
pub(crate) use adapters::wire_isolated_context_with_skill;
pub use adapters::{wire_isolated_context, wire_isolated_context_with_workspace_skills};
pub use adapters::{NoOpCanonicalSessionWriter, ProductionMainContextFactory};
pub use domain::session::{
    DisplayHistoryStepIndexData, DisplayHistoryStepWindowData, SessionListEntryData,
    SessionManagementError, SessionMetadataUpdateData, SessionRestoreStepData,
    SessionResumeViewData,
};
pub use ports::SessionManagementPort;

// Main Session coordinator — cross-BC wiring for Runtime bootstrap.
#[cfg(any(test, feature = "dev"))]
pub use application::test_support;
pub use application::{
    wire_main_session, MainSessionDependencies, MainSessionWiring, MainSessionWiringBuilder,
    OwnedSessionSharedPermit,
};

// 窄 façade根导出（#1022 收口：内部层 mod 私有，跨 BC 消费只经 crate 根与语义模块）
#[cfg(any(test, feature = "dev"))]
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
#[cfg(any(test, feature = "dev"))]
pub(crate) use application::ContextApplicationService;
#[cfg(any(test, feature = "dev"))]
pub(crate) use application::{SessionLoadError, SessionPersistenceService};
#[cfg(any(test, feature = "dev"))]
pub(crate) use domain::session::{
    project_dir_segment, AcceptedInputRecord, ActiveCompactMarker, ChatSegment, CommittedRunSlice,
    CommittedRunStep, FinalizedOutcomeRecord, RunStepCursor, SessionCodec, SessionCodecError,
    SessionCommitPlan, SessionGenerationCodec, SessionGenerationManifest,
    SessionGenerationWireError, SessionHistory, CURRENT_SESSION_SCHEMA_VERSION,
};
pub use domain::session::{CanonicalSession, CommittedStep, CommittedStepMessages, SnapshotState};
#[cfg(any(test, feature = "dev"))]
pub(crate) use domain::ContextMessages;
pub use domain::{
    AcceptedInputAppendData, AcceptedInputError, ContextAppendData, ContextRequestId,
    SessionRevision, SystemBlock, Urgency,
};
pub use domain::{
    AppendReceiptData, CleanupConfirmation, CompactGenerationFailureData,
    CompactGenerationFailureKind, CompactGenerationOutputData, CompactOutcome, CompactRequestData,
    CompactResult, CompactSkipReason, CompactSummaryQuality, CompactTrigger,
    CompactionDecisionData, ContentFingerprint, ContextAppendError, ContextPortError,
    ContextRequestData, ContextWindowData, DecisionReason, FinalizeCause, InvocationReminderData,
    Language, ManualCompactRequestData, RunStepId, SessionId, StepReceiptData,
    SystemPromptSpecData, TaskProgressReminderData, TaskProgressReminderItemData,
    TaskProgressStatus, ToolCallIdentityData, ToolOutcomeKindData, ToolReceiptMutationData,
    ToolReceiptMutationError, ToolReceiptMutationReceiptData, ToolTerminalReceiptData,
};
#[cfg(any(test, feature = "dev"))]
pub(crate) use domain::{ToolCallReceiptData, ToolCallState};
#[cfg(any(test, feature = "dev"))]
pub(crate) use ports::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    PromptMaterializationError, SessionGeneration, SessionRepository, SessionSnapshot,
    SessionSnapshotStore, SessionStoreError, SkillQueryFactory,
};
pub use ports::{ContextPort, MainContextFactory};

pub mod api {
    pub use crate::adapters::migrate_flat_sessions_to_project_dirs;
    pub use crate::domain::session::{DisplayHistoryStepIndexData, DisplayHistoryStepWindowData};
}

pub mod context_port {
    // 消费方（runtime::ports）显式转发集；新增消费先扩本列表。
    #[cfg(any(test, feature = "dev"))]
    pub(crate) use crate::domain::ContextMessage;
    pub use crate::domain::{
        AcceptedInputAppendData, AcceptedInputError, AcceptedInputReceiptData, AppendReceiptData,
        CompactOutcome, CompactRequestData, CompactResult, CompactSkipReason, CompactTrigger,
        CompactionDecisionData, ContentFingerprint, ContextAppendData, ContextAppendError,
        ContextPortError, ContextRequestData, ContextRequestId, ContextWindowData, DecisionReason,
        FinalizeCause, Language, ManualCompactRequestData, RunStepId, SessionId, SessionRevision,
        StepReceiptData, SystemBlock, SystemPromptSpecData, TokenBudget, ToolOutcomeKindData,
        Urgency,
    };
    pub use crate::ports::ContextPort;
}

pub mod compact {
    pub use crate::adapters::compact_summary::{
        messages_selected_for_precompact_memory, CompactGenerator,
    };
    #[cfg(any(test, feature = "dev"))]
    pub(crate) use crate::domain::compact::{
        microcompact_exploration, snip_superseded_exploration, CheckpointSections,
        ContextReadCandidate, ContextReadRun, ContextReadStep, ContinuationCheckpoint,
        ContinuationStatus, ProtectedRunPolicy,
    };
    pub use crate::domain::compact::{
        CompactProgressFn, CompactStageData, CompactTaskBatchStatusData, CompactTaskItemData,
        CompactTaskSnapshotData, CompactTaskStatusData, CompactWorkData,
    };
    #[cfg(any(test, feature = "dev"))]
    pub(crate) use crate::domain::token_budget::{estimate_tokens, summary_budget};
    pub use crate::domain::{estimate_messages_tokens, estimate_tool_schemas_tokens};
}

pub mod guidance {
    pub use crate::adapters::prompt::{
        assess_guidance, init_guidance_dir, resolve_guidance_async, InstructionsLoadedHook,
    };
    #[cfg(any(test, feature = "dev"))]
    pub(crate) use crate::adapters::prompt::{resolve_guidance, universal_execution_discipline};
}

pub mod session {
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
