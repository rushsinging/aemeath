//! `CompactBudgetSources` 的窗口归属：summary 预算归注入窗口，Map 分块归 compact 模型窗口。

use super::*;
use crate::domain::token_budget::{
    compact_chunk_target_tokens, summary_budget, FALLBACK_PREVIOUS_SUMMARY_CAP,
};

#[test]
fn single_window_sources_match_existing_budget_helpers() {
    let sources = CompactBudgetSources::same(100_000);

    assert_eq!(sources.summary_budget(), summary_budget(100_000));
    assert_eq!(
        sources.chunk_target_tokens(),
        compact_chunk_target_tokens(100_000)
    );
    assert_eq!(
        sources.previous_summary_budget(),
        summary_budget(100_000).min(FALLBACK_PREVIOUS_SUMMARY_CAP / 4)
    );
}

#[test]
fn summary_budget_follows_injection_window() {
    let sources = CompactBudgetSources {
        injection_context_size: 200_000,
        compact_context_size: 32_000,
    };

    assert_eq!(sources.summary_budget(), summary_budget(200_000));
    assert_ne!(sources.summary_budget(), summary_budget(32_000));
}

#[test]
fn chunk_target_follows_compact_model_window() {
    let sources = CompactBudgetSources {
        injection_context_size: 512_000,
        compact_context_size: 64_000,
    };

    assert_eq!(
        sources.chunk_target_tokens(),
        compact_chunk_target_tokens(64_000)
    );
    assert_ne!(
        sources.chunk_target_tokens(),
        compact_chunk_target_tokens(512_000)
    );
}

#[test]
fn previous_summary_budget_follows_compact_model_window_with_cap() {
    let sources = CompactBudgetSources {
        injection_context_size: 512_000,
        compact_context_size: 40_000,
    };

    assert_eq!(
        sources.previous_summary_budget(),
        summary_budget(40_000).min(FALLBACK_PREVIOUS_SUMMARY_CAP / 4)
    );
}

#[test]
fn resolve_uses_explicit_compact_window() {
    let sources = CompactBudgetSources::resolve(200_000, Some(32_000));

    assert_eq!(sources.injection_context_size, 200_000);
    assert_eq!(sources.compact_context_size, 32_000);
    assert_eq!(sources.summary_budget(), summary_budget(200_000));
    assert_eq!(
        sources.chunk_target_tokens(),
        compact_chunk_target_tokens(32_000)
    );
}

#[test]
fn resolve_fails_closed_to_injection_window_when_compact_window_missing_or_zero() {
    for compact_context_size in [None, Some(0)] {
        let sources = CompactBudgetSources::resolve(200_000, compact_context_size);

        assert_eq!(
            sources.compact_context_size, 200_000,
            "缺失或非法的 compact 窗口 MUST 回落到注入窗口，而不是放大预算"
        );
        assert_eq!(
            sources.chunk_target_tokens(),
            compact_chunk_target_tokens(200_000)
        );
    }
}
