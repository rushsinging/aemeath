//! Compact 家族子模块（五级管线）。
//!
//! 设计文档：`docs/design/02-modules/context-management/02-compact.md`

mod autocompact;
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
