use crate::domain::{
    CompactionDecision, ContextRequest, DecisionReason, SystemBlock, TokenBudget, Urgency,
};

pub(crate) fn token_budget(
    request: &ContextRequest,
    messages: &crate::domain::ContextMessages,
    system_blocks: &[SystemBlock],
    invocation_reminder: Option<&str>,
) -> TokenBudget {
    let system_tokens = system_blocks
        .iter()
        .map(|block| crate::domain::estimate_tokens(&block.content))
        .sum();
    let message_tokens = messages
        .iter()
        .map(crate::domain::estimate_message_tokens)
        .sum::<usize>()
        + invocation_reminder
            .map(crate::domain::estimate_tokens)
            .unwrap_or_default();
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
/// Two paths, in priority order:
/// 1. **ActualProviderUsage** — when `last_api_total_tokens` is `Some`, the
///    provider-reported total is used directly.  No heuristic projection or
///    delta is applied; the API-reported number already reflects the last
///    turn's real context consumption.
/// 2. **HeuristicFallback** — when no provider usage is available (first turn,
///    or baseline was reset after a compaction / model switch / resume), a
///    full candidate heuristic estimate is built from the current system
///    blocks, messages, and tool schemas.
///
/// Both paths use the same `effective` / `threshold` formula:
/// `effective = context_size - reserved_context(2%) - max_output`
/// `threshold = effective * 0.8`
pub(crate) fn calculate(
    request: &ContextRequest,
    messages: &crate::domain::ContextMessages,
    system_blocks: &[SystemBlock],
    invocation_reminder: Option<&str>,
) -> CompactionDecision {
    let budget = token_budget(request, messages, system_blocks, invocation_reminder);

    let (decision_token_count, reason) = match request.last_api_total_tokens {
        Some(api_total) => (api_total as usize, DecisionReason::ActualProviderUsage),
        None => (budget.total_tokens, DecisionReason::HeuristicFallback),
    };

    let effective =
        crate::domain::effective_context_window(request.context_size, request.max_output_tokens);
    let threshold =
        crate::domain::autocompact_threshold(request.context_size, request.max_output_tokens);

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
        reason,
    }
}
