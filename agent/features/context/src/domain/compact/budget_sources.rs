//! Compact 的窗口预算来源。
//!
//! Compact 涉及两个语义不同的窗口，二者 **MUST** 分开计算：
//!
//! - **注入窗口**：persisted summary 最终注入的主对话模型窗口，决定 `summary_budget`。
//! - **compact 模型窗口**：本次 compact 调用模型（可能与主对话模型不同）的输入窗口，
//!   决定 Map 单块目标与 previous checkpoint 嵌入预算。
//!
//! 把两者合并会让"指定一个窗口更小的 compact 模型"发出超出该模型窗口的请求。

use crate::domain::token_budget::{
    compact_chunk_target_tokens, summary_budget, FALLBACK_PREVIOUS_SUMMARY_CAP,
};

/// Compact 管线使用的两套窗口预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactBudgetSources {
    /// summary 最终注入的对话模型窗口。
    pub injection_context_size: usize,
    /// 本次 compact 调用模型的输入窗口。
    pub compact_context_size: usize,
}

impl CompactBudgetSources {
    /// compact 与主对话共用同一模型（默认语义）时的单一窗口。
    pub fn same(context_size: usize) -> Self {
        Self {
            injection_context_size: context_size,
            compact_context_size: context_size,
        }
    }

    /// 由显式 compact 模型窗口构造预算来源。
    ///
    /// `compact_context_size` 为 `None` 或 `0` 时回落到注入窗口：缺失窗口
    /// **MUST** fail closed，**NEVER** 因为未知而放大预算。
    pub fn resolve(injection_context_size: usize, compact_context_size: Option<usize>) -> Self {
        Self {
            injection_context_size,
            compact_context_size: compact_context_size
                .filter(|size| *size > 0)
                .unwrap_or(injection_context_size),
        }
    }

    /// 持久化 summary 预算：只由注入窗口决定。
    ///
    /// compact 模型窗口更小 **NEVER** 缩小 summary 预算；更小窗口的限制由
    /// [`Self::chunk_target_tokens`] 与 [`Self::previous_summary_budget`] 承担。
    pub fn summary_budget(&self) -> usize {
        summary_budget(self.injection_context_size)
    }

    /// previous checkpoint 嵌入 compact 请求时的预算：受 compact 模型窗口约束。
    pub fn previous_summary_budget(&self) -> usize {
        summary_budget(self.compact_context_size).min(FALLBACK_PREVIOUS_SUMMARY_CAP / 4)
    }

    /// Map 单块目标 token 数：受 compact 模型窗口约束。
    pub fn chunk_target_tokens(&self) -> usize {
        compact_chunk_target_tokens(self.compact_context_size)
    }
}

#[cfg(test)]
#[path = "budget_sources_tests.rs"]
mod budget_sources_tests;
