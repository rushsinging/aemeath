use crate::domain::{
    CompactionDecision, ContextMessage, ContextRequest, DecisionReason, SystemBlock, TokenBudget,
    Urgency,
};

pub(crate) fn token_budget(
    request: &ContextRequest,
    messages: &[ContextMessage],
    system_blocks: &[SystemBlock],
) -> TokenBudget {
    let system_tokens = system_blocks
        .iter()
        .map(|block| crate::domain::estimate_tokens(&block.content))
        .sum();
    let message_tokens = crate::domain::estimate_messages_tokens(messages);
    let tool_schema_tokens = request.tool_schema_tokens;
    TokenBudget {
        system_tokens,
        tool_schema_tokens,
        message_tokens,
        total_tokens: system_tokens + tool_schema_tokens + message_tokens,
    }
}

pub(crate) fn calculate(
    request: &ContextRequest,
    messages: &[ContextMessage],
    system_blocks: &[SystemBlock],
) -> CompactionDecision {
    let budget = token_budget(request, messages, system_blocks);
    let system_delta = budget
        .system_tokens
        .saturating_sub(request.prev_system_tokens.unwrap_or_default());
    let tool_delta = budget
        .tool_schema_tokens
        .saturating_sub(request.prev_tool_schema_tokens.unwrap_or_default());
    let pending_delta = crate::domain::estimate_messages_tokens(&request.pending_messages);
    let (estimated_tokens, reason) = match request.last_api_input_tokens {
        Some(previous) => (
            previous as usize + pending_delta + system_delta + tool_delta,
            DecisionReason::ActualApiWithDelta,
        ),
        None => (budget.total_tokens, DecisionReason::Heuristic),
    };
    let effective =
        crate::domain::effective_context_window(request.context_size, request.max_output_tokens);
    let threshold =
        crate::domain::autocompact_threshold(request.context_size, request.max_output_tokens);
    let percentage = estimated_tokens.saturating_mul(100) / effective.max(1);
    let urgency = match percentage {
        0..=69 => Urgency::None,
        70..=79 => Urgency::Monitor,
        80..=89 => Urgency::Should,
        _ => Urgency::Must,
    };
    CompactionDecision {
        needed: estimated_tokens > threshold,
        urgency,
        estimated_tokens,
        threshold,
        reason,
    }
}
