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
//! - HookPort -> hook BC (#922)
//! - ReasoningPort -> workflow BC (#919)
//! - RuntimeStreamEvent::Usage 路径 -> audit BC (#927)

pub mod context_port;
pub mod policy_port;
pub mod provider_factory;
pub mod provider_port;
pub mod session_query;
pub mod tool_result_blob;

pub use context_port::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactTrigger, ContentFingerprint, ContextAppend, ContextAppendError,
    ContextPort, ContextPortError, ContextRequest, ContextRequestId, ContextWindow, FinalizeCause,
    Language, ManualCompactRequest, RunStepId, SessionId, SessionRevision, StepReceipt,
    SystemPromptSpec,
};
#[cfg(test)]
pub use context_port::{
    CompactResult, CompactSkipReason, CompactionDecision, DecisionReason, SystemBlock, TokenBudget,
    ToolOutcomeKind, Urgency,
};
pub use policy_port::PolicyPort;
#[cfg(test)]
pub use policy_port::{PolicyDecision, PolicyRequest};
pub use provider_factory::{ProviderBinding, ProviderBuildSpec, ProviderFactory};
pub use provider_port::{
    InvocationOptions, InvocationRequest, ModelId, ModelToolSchema, ProviderPort, RawUsageSnapshot,
    RequestSystemBlock, StopReason,
};
#[cfg(test)]
pub use provider_port::{InvocationStream, ModelCapability, ProviderError, ReasoningCapability};
pub use session_query::SessionQueryPort;
pub use tool_result_blob::{ToolResultBlobError, ToolResultBlobPort, ToolResultBlobRef};
