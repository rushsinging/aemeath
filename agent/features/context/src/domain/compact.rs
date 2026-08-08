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
    autocompact_threshold, compaction_urgency, effective_context_window, estimate_json_tokens,
    estimate_message_tokens, estimate_messages_tokens, estimate_tokens, estimate_tokens_with_ratio,
    estimate_tool_schemas_tokens, needs_compaction, needs_compaction_actual, needs_compaction_full,
    needs_compaction_total, needs_compaction_with_output, summary_budget,
};
pub use autocompact::*;
pub use continuation_checkpoint::*;
pub use microcompact::{microcompact_chain, microcompact_messages};
pub use restore::*;

/// Compact 进度阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactStage {
    Preparing,
    Summarizing,
    Finalizing,
}

impl CompactStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Summarizing => "summarizing",
            Self::Finalizing => "finalizing",
        }
    }
}

/// Compact 进度回调（domain 单一真相，#1500 由 adapters 上移）。
///
/// `compact_messages_with_llm` 在各阶段（Preparing/Summarizing/Finalizing）
/// 调用此回调通知调用方。map-reduce 模式下，每个 chunk 处理前也会调用，
/// 携带 `(current, total)` chunk 计数。闭包形式可自动实现（F: Fn）。
pub trait CompactProgressFn: Send + Sync {
    fn emit(&self, stage: CompactStage, current: Option<usize>, total: Option<usize>);
}

impl<F> CompactProgressFn for F
where
    F: Fn(CompactStage, Option<usize>, Option<usize>) + Send + Sync,
{
    fn emit(&self, stage: CompactStage, current: Option<usize>, total: Option<usize>) {
        self(stage, current, total)
    }
}
