//! Context：会话上下文的编排、持久化与压缩决策。
//!
//! # Published Language（DDD 五类判据，#1863 收口）
//!
//! | 组 | 实体 | 消费者 |
//! |---|---|---|
//! | 命令 | `ContextRequestData`/`ContextAppendData`/`CompactRequestData`/`ManualCompactRequestData`/`ToolReceiptMutationData`/`AcceptedInputAppendData`/`SessionMetadataUpdateData` | runtime、sdk |
//! | 事件 | `StepReceiptData`/`AppendReceiptData`/`AcceptedInputReceiptData`/`ToolCallReceiptData`/`ToolTerminalReceiptData`/`ToolReceiptMutationReceiptData`/`CompactResult`/`CompactOutcome`/`CompactionDecisionData`/`CleanupConfirmation` | runtime、sdk、cli |
//! | 值对象 | Session/Compact/Reminder/Tool 展示与决策 Data（含 `CanonicalSession` 持久化 DTO 身份、`TokenBudget`、`error_codes` 细分码词汇） | runtime、sdk、cli、composition |
//! | 端口 | `ContextPort`、`SessionManagementPort`、`MainContextFactory`、`CompactGenerator`、`MainSessionWiring`（+`MainSessionDependencies`/`Builder`/`OwnedSessionSharedPermit`） | runtime、composition、sdk |
//! | 工厂 | `wire_main_session`（出口 `SdkError`）、`wire_session_management`、`wire_production_main_context_factory`、`wire_isolated_context{,_with_workspace_skills}` | composition |
//!
//! 子发布语言模块（模块=1 计数）：`api` / `compact` / `guidance` / `session`。
//! 错误跨界唯一 `share::error::DomainError`（5 边界枚举已折叠；`MainSessionError` 内部化）。
//!
//! #1863 收口判定：
//! - main_session 穿透清零（Gate/`BoundMainRun`/`MainSessionError` 不再经根面）；
//! - 6 个 adapter 实现体（`ProductionMainContextFactory`/`DatasetSessionManagement`/
//!   `DatasetCanonicalSessionWriter`/`AtomicBlobAcceptedInputWriter`/
//!   `AtomicBlobToolReceiptWriter`/`WorkspaceSkillQueryFactory`）`pub(crate)`，
//!   composition 仅经 wire 工厂装配；
//! - `NoOpCanonicalSessionWriter`/`test_support` 归 `cfg(any(test, feature="dev"))`；
//! - `AtomicBlobSessionManagement` 仍根导出（legacy 读路径，随后续迁移收窄）。

/// Context Management crate — 对话历史容器、上下文压缩、token 预算、提示组装、记忆注入。
///
/// 设计文档：`docs/design/02-modules/context-management/README.md`
mod constants;
pub use constants::error_codes;
pub(crate) use constants::LOG_TARGET;

mod adapters;
// dev 门控契约测试经 crate 根消费（canonical_session_repository / isolated_context_with_skill）。
#[cfg(any(test, feature = "dev"))]
// workspace feature 统一下 dev 门控测试不参与 clippy 编译，显式标注未用豁免。
#[allow(unused_imports)]
pub(crate) use adapters::capture_session_lifecycle;
mod application;
mod domain;
mod ports;

#[cfg(any(test, feature = "dev"))]
#[cfg(test)]
pub(crate) use adapters::wire_isolated_context_with_skill;
pub use adapters::{
    wire_isolated_context, wire_isolated_context_with_workspace_skills,
    wire_production_main_context_factory, wire_session_management,
};
pub use domain::reminder::{
    render_invocation_reminder_body, CompactBehavior, InjectBehavior, RefreshTrigger,
    ReminderDedup, ReminderEventSource, ReminderKind, ReminderPlacement, ReminderPolicy,
    ReminderPriority, ReminderSnapshot, ReminderSource,
};
pub use domain::session::{
    SessionListEntryData, SessionMetadataUpdateData, SessionRestoreStepData, SessionResumeViewData,
};
pub use ports::SessionManagementPort;

// Main Session coordinator — cross-BC wiring for Runtime bootstrap.
#[cfg(any(test, feature = "dev"))]
pub use adapters::NoOpCanonicalSessionWriter;
#[cfg(any(test, feature = "dev"))]
pub use application::test_support;
pub use application::{
    wire_main_session, MainSessionDependencies, MainSessionWiring, MainSessionWiringBuilder,
    OwnedSessionSharedPermit,
};

// 窄 façade根导出（#1022 收口：内部层 mod 私有，跨 BC 消费只经 crate 根与语义模块）
pub use adapters::AtomicBlobSessionManagement;
#[cfg(test)]
pub(crate) use adapters::{
    decode_session, skill_prompt_budget, AcceptedInputWriter, AtomicBlobCanonicalSessionWriter,
    AtomicBlobSessionStore, CanonicalSessionRepository, CanonicalSessionWriter,
    CommittedMemoryRetrieveAdapter, DatasetSessionReader, InMemorySessionRepository,
    LegacySessionDecoder, NoOpContextMemorySource, SkillPromptSource, ToolReceiptWriter,
};
#[cfg(test)]
pub(crate) use adapters::{
    DatasetCanonicalSessionWriter, ProductionMainContextFactory, WorkspaceSkillQueryFactory,
};
#[cfg(test)]
pub(crate) use application::ContextApplicationService;
#[cfg(test)]
pub(crate) use application::{SessionLoadError, SessionPersistenceService};
#[cfg(test)]
pub(crate) use domain::session::{
    project_dir_segment, AcceptedInputRecord, ActiveCompactMarker, CanonicalSession, ChatSegment,
    CommittedRunSlice, CommittedRunStep, CommittedStep, CommittedStepMessages,
    FinalizedOutcomeRecord, RunStepCursor, SessionCodec, SessionCodecError, SessionCommitPlan,
    SessionGenerationCodec, SessionGenerationManifest, SessionGenerationWireError, SessionHistory,
    SnapshotState, CURRENT_SESSION_SCHEMA_VERSION,
};
#[cfg(test)]
pub(crate) use domain::ContextMessages;
pub use domain::{
    AcceptedInputAppendData, AcceptedInputReceiptData, ContextAppendData, ContextRequestId,
    SessionRevision, SystemBlock, Urgency,
};
pub use domain::{
    AppendReceiptData, BackgroundProcessCompletionStatus, BackgroundProcessReminderItemData,
    CleanupConfirmation, CompactGenerationFailureData, CompactGenerationFailureKind,
    CompactGenerationOutputData, CompactOutcome, CompactRequestData, CompactResult,
    CompactSkipReason, CompactSummaryQuality, CompactTrigger, CompactionDecisionData,
    ContentFingerprint, ContextRequestData, ContextWindowData, DecisionReason, FinalizeCause,
    InvocationReminderData, Language, ManualCompactRequestData, MemoryRecallEntryData,
    StepReceiptData, SystemPromptSpecData, TaskProgressReminderData, TaskProgressReminderItemData,
    TaskProgressStatus, TokenBudget, ToolCallIdentityData, ToolCallReceiptData,
    ToolOutcomeKindData, ToolReceiptMutationData, ToolReceiptMutationReceiptData, ToolSchemaData,
    ToolTerminalReceiptData,
};
#[cfg(test)]
pub(crate) use ports::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    SessionGeneration, SessionRepository, SessionSnapshot, SessionSnapshotStore, SessionStoreError,
    SkillQueryFactory,
};
pub use ports::{ContextPort, MainContextFactory};

pub mod api {
    pub use crate::adapters::migrate_flat_sessions_to_project_dirs;
    pub use crate::domain::session::{DisplayHistoryStepIndexData, DisplayHistoryStepWindowData};
}

pub mod compact {
    pub use crate::adapters::compact_summary::messages_selected_for_precompact_memory;
    #[cfg(test)]
    pub(crate) use crate::domain::compact::{
        microcompact_exploration, snip_superseded_exploration, CheckpointSections,
        ContextReadCandidate, ContextReadRun, ContextReadStep, ContinuationCheckpoint,
        ContinuationStatus, ProtectedRunPolicy,
    };
    pub use crate::domain::compact::{
        CompactProgressFn, CompactStageData, CompactTaskBatchStatusData, CompactTaskItemData,
        CompactTaskSnapshotData, CompactTaskStatusData, CompactWorkData,
    };
    #[cfg(test)]
    pub(crate) use crate::domain::token_budget::{estimate_tokens, summary_budget};
    pub use crate::domain::{estimate_messages_tokens, estimate_tool_schemas_tokens};
    pub use crate::ports::CompactGenerator;
}

pub mod guidance {
    pub use crate::adapters::prompt::{
        assess_guidance, init_guidance_dir, resolve_guidance_async, InstructionsLoadedHook,
    };
    #[cfg(test)]
    pub(crate) use crate::adapters::prompt::{resolve_guidance, universal_execution_discipline};
}

pub mod session {
    pub use crate::domain::session::{CanonicalSession, SessionMetadata, SnapshotState};
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
    mod injection_freeze;
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
    mod wire_factories;
}
