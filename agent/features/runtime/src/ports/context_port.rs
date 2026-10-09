//! Runtime 只消费 Context Management 发布的 OHS；本地不重复定义契约。

pub use context::{
    AcceptedInputAppendData, AcceptedInputReceiptData, AppendReceiptData, CompactOutcome,
    CompactRequestData, CompactResult, CompactSkipReason, CompactTrigger, CompactionDecisionData,
    ContentFingerprint, ContextAppendData, ContextPort, ContextRequestData, ContextRequestId,
    ContextWindowData, DecisionReason, FinalizeCause, Language, ManualCompactRequestData,
    SessionRevision, StepReceiptData, SystemBlock, SystemPromptSpecData, TokenBudget,
    ToolOutcomeKindData, ToolSchemaData, Urgency,
};
pub use share::error::DomainError;
pub use share::ids::{RunStepId, SessionId};

#[cfg(test)]
#[path = "context_port_tests.rs"]
mod tests;
