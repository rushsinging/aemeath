use crate::domain::{
    CompactionDecision, ContextRequest, DecisionReason, SystemBlock, TokenBudget, Urgency,
    MIN_EFFECTIVE_WINDOW,
};

/// Heuristic 校准系数的有效区间（#1626）。
/// 区间外的值视为不可信观测，按 1.0（不校准）处理。
pub(crate) const HEURISTIC_CALIBRATION_RANGE: std::ops::RangeInclusive<f64> = 0.5..=2.0;

/// 解析 request 携带的 heuristic 校准系数；越界或缺失按 1.0。
pub(crate) fn resolve_calibration_factor(request: &ContextRequest) -> f64 {
    request
        .heuristic_calibration
        .filter(|factor| HEURISTIC_CALIBRATION_RANGE.contains(factor))
        .unwrap_or(1.0)
}

pub(crate) fn token_budget(
    request: &ContextRequest,
    messages: &crate::domain::ContextMessages,
    system_blocks: &[SystemBlock],
) -> TokenBudget {
    let system_tokens = system_blocks
        .iter()
        .map(|block| crate::domain::estimate_tokens(&block.content))
        .sum();
    let message_tokens = messages
        .iter()
        .map(crate::domain::estimate_message_tokens)
        .sum::<usize>();
    let tool_schema_tokens = request.tool_schema_tokens;
    TokenBudget {
        system_tokens,
        tool_schema_tokens,
        message_tokens,
        total_tokens: system_tokens + tool_schema_tokens + message_tokens,
    }
}

/// Compute the compaction decision.
///
/// Three paths, in priority order:
/// 0. **MisconfiguredWindow** — when the effective window is below
///    [`MIN_EFFECTIVE_WINDOW`] even after the output-reservation clamp,
///    the context window itself is misconfigured. Autocompact is disabled
///    (a permanent trigger would storm the circuit breaker) and a warning
///    is logged (#1626).
/// 1. **ActualProviderUsage** — when `last_api_total_tokens` is `Some`, the
///    provider-reported total is used directly.  No heuristic view or
///    delta is applied; the API-reported number already reflects the last
///    run step's real context consumption.
/// 2. **HeuristicFallback** — when no provider usage is available (first run step,
///    or baseline was reset after a compaction / model switch / resume), a
///    full candidate heuristic estimate is built from the current system
///    blocks, messages, and tool schemas, scaled by the sliding calibration
///    factor (`heuristic_calibration`) when one is available (#1626).
///
/// All paths use the same `effective` / `threshold` formula:
/// `effective = context_size - reserved_context(2%) - clamped_max_output(≤25% window)`
/// `threshold = effective * 0.8`
pub(crate) fn calculate(
    request: &ContextRequest,
    messages: &crate::domain::ContextMessages,
    system_blocks: &[SystemBlock],
) -> CompactionDecision {
    let budget = token_budget(request, messages, system_blocks);

    let effective =
        crate::domain::effective_context_window(request.context_size, request.max_output_tokens);
    let threshold =
        crate::domain::autocompact_threshold(request.context_size, request.max_output_tokens);

    if effective < MIN_EFFECTIVE_WINDOW {
        log::warn!(
            target: crate::LOG_TARGET,
            "[autocompact] 窗口配置疑似错误：context_size {} 经 max_output 预留 clamp（≤25% 窗口）后 effective 仅 {} tokens（下限 {}）——auto-compact 已禁用以避免压缩风暴，请检查 context_size / max_output_tokens 配置",
            request.context_size,
            effective,
            MIN_EFFECTIVE_WINDOW,
        );
        return CompactionDecision {
            needed: false,
            urgency: Urgency::None,
            decision_token_count: budget.total_tokens,
            threshold,
            context_size: request.context_size,
            effective_window: effective,
            reason: DecisionReason::MisconfiguredWindow,
        };
    }

    let (decision_token_count, reason) = match request.last_api_total_tokens {
        Some(api_total) => (api_total as usize, DecisionReason::ActualProviderUsage),
        None => (
            (budget.total_tokens as f64 * resolve_calibration_factor(request)) as usize,
            DecisionReason::HeuristicFallback,
        ),
    };

    let percentage = decision_token_count.saturating_mul(100) / effective.max(1);
    let urgency = match percentage {
        0..=69 => Urgency::None,
        70..=79 => Urgency::Monitor,
        80..=89 => Urgency::Should,
        _ => Urgency::Must,
    };

    CompactionDecision {
        needed: decision_token_count > threshold,
        urgency,
        decision_token_count,
        threshold,
        context_size: request.context_size,
        effective_window: effective,
        reason,
    }
}
