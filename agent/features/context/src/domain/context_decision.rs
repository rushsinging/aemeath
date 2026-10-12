use super::constants::{HEURISTIC_CALIBRATION_RANGE, PROVIDER_USAGE_TRUST_RATIO_RANGE};
use crate::domain::{
    CompactionDecisionData, ContextRequestData, DecisionReason, SystemBlock, TokenBudget, Urgency,
    MIN_EFFECTIVE_WINDOW,
};

/// 解析 request 携带的 heuristic 校准系数；越界或缺失按 1.0。
pub(crate) fn resolve_calibration_factor(request: &ContextRequestData) -> f64 {
    request
        .heuristic_calibration
        .filter(|factor| HEURISTIC_CALIBRATION_RANGE.contains(factor))
        .unwrap_or(1.0)
}

pub(crate) fn token_budget(
    request: &ContextRequestData,
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
/// Paths, in priority order:
/// 0. **MisconfiguredWindow** — when the effective window is below
///    [`MIN_EFFECTIVE_WINDOW`] even after the output-reservation clamp,
///    the context window itself is misconfigured. Autocompact is disabled
///    (a permanent trigger would storm the circuit breaker) and a warning
///    is logged (#1626).
/// 1. **ActualProviderUsage** — when `last_api_total_tokens` is `Some` and
///    `api / heuristic` 落在可信区间 [`PROVIDER_USAGE_TRUST_RATIO_RANGE`]
///    （默认 [0.5, 2.0]）内，采信 API 上报占用做预判。
/// 2. **HeuristicPreferred** — API 存在但相对本地估算越界（脏 usage），
///    预判改用本地估算；真正超限仍由 `ContextTooLong` 硬触发 compact。
/// 3. **HeuristicFallback** — 无 provider usage（首步 / compact 后 reset /
///    换模型 / resume）时，用本地估算 × `heuristic_calibration`。
///
/// All paths use the same `effective` / `threshold` formula:
/// `effective = context_size - reserved_context(2%) - clamped_max_output(≤25% window)`
/// `threshold = effective * ratio`（ratio 来自 config_snapshot 的
/// `auto_compact_threshold_ratio`，默认 0.8，clamp 到 [0.5, 0.95]）
pub(crate) fn calculate(
    request: &ContextRequestData,
    messages: &crate::domain::ContextMessages,
    system_blocks: &[SystemBlock],
) -> CompactionDecisionData {
    let budget = token_budget(request, messages, system_blocks);

    let effective =
        crate::domain::effective_context_window(request.context_size, request.max_output_tokens);
    let threshold = crate::domain::autocompact_threshold(
        request.context_size,
        request.max_output_tokens,
        request.config_snapshot.auto_compact_threshold_ratio(),
    );

    if effective < MIN_EFFECTIVE_WINDOW {
        log::warn!(
            target: crate::LOG_TARGET,
            "[autocompact] 窗口配置疑似错误：context_size {} 经 max_output 预留 clamp（≤25% 窗口）后 effective 仅 {} tokens（下限 {}）——auto-compact 已禁用以避免压缩风暴，请检查 context_size / max_output_tokens 配置",
            request.context_size,
            effective,
            MIN_EFFECTIVE_WINDOW,
        );
        return CompactionDecisionData {
            needed: false,
            urgency: Urgency::None,
            decision_token_count: budget.total_tokens,
            threshold,
            context_size: request.context_size,
            effective_window: effective,
            reason: DecisionReason::MisconfiguredWindow,
        };
    }

    let heuristic_tokens =
        (budget.total_tokens as f64 * resolve_calibration_factor(request)) as usize;
    let (decision_token_count, reason) = match request.last_api_total_tokens {
        None => (heuristic_tokens, DecisionReason::HeuristicFallback),
        Some(api_total) if heuristic_tokens == 0 => {
            (api_total as usize, DecisionReason::ActualProviderUsage)
        }
        Some(api_total) => {
            let ratio = api_total as f64 / heuristic_tokens as f64;
            if PROVIDER_USAGE_TRUST_RATIO_RANGE.contains(&ratio) {
                (api_total as usize, DecisionReason::ActualProviderUsage)
            } else {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[autocompact] provider usage 相对本地估算越界，改用本地预判：api={api_total} heuristic={heuristic_tokens} ratio={ratio:.3} trust=[{:?}, {:?}]",
                    PROVIDER_USAGE_TRUST_RATIO_RANGE.start(),
                    PROVIDER_USAGE_TRUST_RATIO_RANGE.end(),
                );
                (heuristic_tokens, DecisionReason::HeuristicPreferred)
            }
        }
    };

    let percentage = decision_token_count.saturating_mul(100) / effective.max(1);
    let urgency = match percentage {
        0..=69 => Urgency::None,
        70..=79 => Urgency::Monitor,
        80..=89 => Urgency::Should,
        _ => Urgency::Must,
    };

    CompactionDecisionData {
        needed: decision_token_count > threshold,
        urgency,
        decision_token_count,
        threshold,
        context_size: request.context_size,
        effective_window: effective,
        reason,
    }
}
