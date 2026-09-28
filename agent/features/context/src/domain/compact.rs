//! Compact 家族子模块（五级管线）。
//!
//! 设计文档：`docs/design/02-modules/context-management/02-compact.md`

mod autocompact;
mod budget_sources;
mod context_read_candidate;
mod continuation_checkpoint;
mod microcompact;
mod restore;
mod snip;
mod structured_facts;

// 显式 re-export token_budget 的预算/估算函数（#1486：排除
// FALLBACK_PREVIOUS_SUMMARY_CAP，避免与 compact_summary 的 glob
// re-export 产生歧义——该常量由 compact_summary 单点导出）。
#[cfg(test)]
// workspace feature 统一下 dev 门控测试不参与 clippy 编译，显式标注未用豁免。
#[allow(unused_imports)]
pub use crate::domain::token_budget::{estimate_messages_tokens, estimate_tool_schemas_tokens};
pub use autocompact::*;
pub use budget_sources::CompactBudgetSources;
pub use context_read_candidate::{ContextReadCandidate, ProtectedRunPolicy};
#[cfg(test)]
pub use context_read_candidate::{ContextReadRun, ContextReadStep};
pub use continuation_checkpoint::*;
pub use microcompact::microcompact_exploration;
pub use restore::*;
pub use snip::snip_superseded_exploration;
pub use structured_facts::*;

/// Compact 操作阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactStageData {
    Preparing,
    Generating,
    Mapping,
    Reducing,
    Refreshing,
    Finalizing,
}

impl CompactStageData {
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
pub enum CompactWorkData {
    Indeterminate,
    Determinate { completed: usize, total: usize },
}

/// Compact 进度回调（domain 单一真相）。
pub trait CompactProgressFn: Send + Sync {
    fn emit(&self, stage: CompactStageData, work: CompactWorkData);
}

impl<F> CompactProgressFn for F
where
    F: Fn(CompactStageData, CompactWorkData) + Send + Sync,
{
    fn emit(&self, stage: CompactStageData, work: CompactWorkData) {
        self(stage, work)
    }
}
