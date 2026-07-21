//! Runtime 出站端口定义。
//!
//! 本模块定义 Runtime 八模块消费的全部出站 Port trait 和 Published Language 类型。
//! 设计来源：`docs/design/02-modules/runtime/06-ports-and-adapters.md`。
//!
//! #873 建立骨架；#901 细化 ProviderPort PL（冻结契约）。
//!
//! 各 Port 对应的 BC 负责细化 PL 类型行为，后续迁移到各自 crate：
//! - ContextPort -> context BC (#868)
//! - ProviderPort -> provider BC (#901) ✅ PL 已冻结
//! - ToolCatalogPort / ToolExecutionPort -> tools BC (#908)
//! - PolicyPort -> policy BC (#917)
//! - MemoryPort -> memory BC (#897) ✅ Port 由 memory crate 提供，runtime 通过 `memory::api::MemoryPort` 消费
//! - TaskPort -> task BC (#885)
//! - WorkspacePort -> project BC (#892)
//! - HookPort -> hook BC (#922)
//! - ReasoningPort -> workflow BC (#919)
//! - UsageSink -> audit BC (#927)
//! - EventSink / InputBuffer -> runtime 内部

pub mod context_port;
pub mod event_sink;
pub mod input_buffer;
pub mod legacy;
pub mod policy_port;
pub mod provider_factory;
pub mod provider_port;
pub mod task_port;
pub mod tool_port;
pub mod tool_result_blob;
pub mod usage_sink;
pub mod workspace_port;

pub use context_port::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CalendarDate,
    CompactOutcome, CompactRequest, CompactResult, CompactSkipReason, CompactTrigger,
    CompactionDecision, ContentFingerprint, ContextAppend, ContextAppendError, ContextMessage,
    ContextPort, ContextPortError, ContextRequest, ContextRequestId, ContextWindow, DecisionReason,
    FinalizeCause, Language, ManualCompactRequest, RunStepId, SessionId, SessionRevision,
    StepReceipt, SystemBlock, SystemPromptSpec, TaskReminderSnapshot, TokenBudget, ToolOutcomeKind,
    Urgency,
};
pub use event_sink::EventSink;
pub use hook::api::{HookInvocation, HookOutcome, HookPoint, HookPort};
pub use input_buffer::InputBuffer;
pub(crate) use input_buffer::{RuntimeInputEventDrainPort, RuntimeQueueDrainPort};
pub use policy_port::{
    ApprovalSubject, PolicyDecision, PolicyMode, PolicyPort, PolicyReason, PolicyRequest,
    PolicyRequestError,
};
pub use provider_factory::{ProviderBinding, ProviderBuildSpec, ProviderFactory};
pub use provider_port::{
    InvocationDelta, InvocationEvent, InvocationOptions, InvocationRequest, InvocationStream,
    ModelCapability, ModelId, ModelToolSchema, ProviderCompletion, ProviderContentBlock,
    ProviderError, ProviderErrorKind, ProviderPort, ProviderToolCall, ProviderToolCallId,
    RawUsageSnapshot, ReasoningCapability, ReasoningLevel, ReasoningMappingKind,
    RequestSystemBlock, StopReason,
};
pub use task_port::TaskPort;
pub use tool_port::{
    RegistryScopeName, ToolCatalogPort, ToolCatalogSnapshot, ToolExecutionPort, ToolInvocation,
    ToolOutcome, ToolProfileName,
};
pub use tool_result_blob::{ToolResultBlobError, ToolResultBlobPort, ToolResultBlobRef};
pub use usage_sink::{UsageDropReason, UsageEmitOutcome, UsageRecord, UsageSink};
pub use workspace_port::{WorkspaceFrame, WorkspacePort};
