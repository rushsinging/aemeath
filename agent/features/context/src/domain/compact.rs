//! Compact 家族子模块（五级管线）。
//!
//! 设计文档：`docs/design/02-modules/context-management/02-compact.md`

mod autocompact;
mod continuation_checkpoint;
mod microcompact;
mod restore;

// 显式 re-export token_budget 的预算/估算函数（#1486：排除
// FALLBACK_PREVIOUS_SUMMARY_CAP，避免与 compact_summary 的 glob
// re-export 产生歧义——该常量由 compact_summary 单点导出）。
pub use crate::domain::token_budget::{
    autocompact_threshold, effective_context_window, estimate_json_tokens, estimate_message_tokens,
    estimate_messages_tokens, estimate_tokens, estimate_tokens_with_ratio,
    estimate_tool_schemas_tokens, summary_budget,
};
pub use autocompact::*;
pub use continuation_checkpoint::*;
pub use microcompact::{microcompact_chain, microcompact_messages};
pub use restore::*;

/// Compact 操作阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactStage {
    Preparing,
    Generating,
    Mapping,
    Reducing,
    Refreshing,
    Finalizing,
}

impl CompactStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Generating => "generating",
            Self::Mapping => "mapping",
            Self::Reducing => "reducing",
            Self::Refreshing => "refreshing",
            Self::Finalizing => "finalizing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactWork {
    Indeterminate,
    Determinate { completed: usize, total: usize },
}

/// Compact 进度回调（domain 单一真相）。
pub trait CompactProgressFn: Send + Sync {
    fn emit(&self, stage: CompactStage, work: CompactWork);
}

impl<F> CompactProgressFn for F
where
    F: Fn(CompactStage, CompactWork) + Send + Sync,
{
    fn emit(&self, stage: CompactStage, work: CompactWork) {
        self(stage, work)
    }
}
