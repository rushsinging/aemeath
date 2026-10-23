//! Token estimation service for context management
//!
//! Provides CJK-aware token estimation for messages and text content.
//! Note: This uses estimation algorithms, not actual tokenizers.
//! For more accurate results, consider integrating tiktoken.

pub use super::constants::MIN_EFFECTIVE_WINDOW;
pub(crate) use super::constants::{
    COMPACT_TAIL_WINDOW_PERCENT, FALLBACK_PREVIOUS_SUMMARY_CAP, MAX_OUTPUT_WINDOW_RATIO_CAP,
    TOKENS_PER_CJK_CHAR, TOKENS_PER_LATIN_CHAR,
};
use share::message::{ContentBlock, Message};

// ── 预算与估算函数 ──────────────────────────────────────────────
// （历史遗留的 TokenEstimation / ContextUsage 包装类型无任何消费者，
// 已在 #1486 修复中删除；以下为仍在使用的纯函数。）

/// Estimate token count for a string.
/// CJK-aware：中文字符 ≈ 0.6 token/字，英文字符 ≈ 0.3 token/字。
pub fn estimate_tokens(text: &str) -> usize {
    estimate_tokens_with_char_rates(text, TOKENS_PER_CJK_CHAR, TOKENS_PER_LATIN_CHAR)
}

/// Estimate tokens with custom per-character rates（CJK / 非 CJK）。
pub fn estimate_tokens_with_char_rates(
    text: &str,
    tokens_per_cjk_char: f64,
    tokens_per_latin_char: f64,
) -> usize {
    let mut cjk_chars = 0usize;
    let mut latin_chars = 0usize;

    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk_chars += 1;
        } else {
            latin_chars += 1;
        }
    }

    let cjk_rate = tokens_per_cjk_char.clamp(0.1, 2.0);
    let latin_rate = tokens_per_latin_char.clamp(0.1, 2.0);
    ((cjk_chars as f64) * cjk_rate + (latin_chars as f64) * latin_rate).ceil() as usize
}

/// 兼容旧调用：`bytes_per_token` 换算为拉丁字符比率 `1 / bytes_per_token`，
/// CJK 仍用默认 0.6。新代码应优先 [`estimate_tokens`] /
/// [`estimate_tokens_with_char_rates`]。生产路径走默认比率；仅历史测试引用。
#[cfg(test)]
pub fn estimate_tokens_with_ratio(text: &str, bytes_per_token: f64) -> usize {
    let ratio = bytes_per_token.clamp(2.0, 6.0);
    estimate_tokens_with_char_rates(text, TOKENS_PER_CJK_CHAR, 1.0 / ratio)
}

/// Check if a character is in CJK Unicode ranges.
fn is_cjk_char(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Unified Ideographs Extension A
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        | '\u{3000}'..='\u{303F}' // CJK Symbols and Punctuation
        | '\u{FF00}'..='\u{FFEF}' // Fullwidth Forms
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
    )
}

/// Estimate tokens for JSON content（与文本相同：按字符比率粗估）。
pub fn estimate_json_tokens(text: &str) -> usize {
    estimate_tokens(text)
}

/// Estimate total tokens in a message list
pub fn estimate_messages_tokens(messages: &[Message]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Estimate tokens for a single message
pub fn estimate_message_tokens(message: &Message) -> usize {
    // ~4 tokens overhead per message (role, formatting)
    4 + message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => estimate_tokens(text),
            ContentBlock::ToolUse { name, input, .. } => {
                estimate_tokens(name) + estimate_json_tokens(&input.to_string())
            }
            ContentBlock::ToolResult { content, .. } => match content {
                serde_json::Value::String(s) => estimate_tokens(s),
                _ => estimate_tokens(&content.to_string()),
            },
            ContentBlock::Image { .. } => 85, // ~85 tokens overhead for image reference
            ContentBlock::Thinking { thinking, .. } => estimate_tokens(thinking),
            ContentBlock::Reminder {
                kind,
                version,
                at,
                seq,
                body,
            } => {
                // 与 to_llm_view 一致：按出站 <system-reminder> envelope 文本估算。
                estimate_tokens(&ContentBlock::reminder_to_llm_envelope(
                    kind, *version, at, *seq, body,
                ))
            }
        })
        .sum::<usize>()
}

// ---- Autocompact threshold constants ----
// effective = context_size - reserved_context(2%) - clamped_max_output(≤25% 窗口)
// threshold = effective * ratio（默认 0.8；配置化见 `autocompact_threshold`）

/// max_output 预留 clamp 到窗口比例上限（#1626 短窗口护栏）。
pub fn clamped_max_output(context_size: usize, max_output_tokens: usize) -> usize {
    max_output_tokens
        .min(context_size / MAX_OUTPUT_WINDOW_RATIO_CAP)
        .max(1)
}

/// Reserved context for guidance and compaction summary.
/// 预留上下文预算：context window 的 5%（#1773 由 2% 放宽，让压缩后的
/// 摘要能承载更多历史）。
pub fn summary_budget(context_size: usize) -> usize {
    context_size / 20
}

/// 自动 Memory 注入的 token 预算：窗口的 2%（#1777）。
///
/// 取代旧的固定值——固定预算在大窗口浪费空间、在小窗口可能超支。2% 恰好
/// 可用整数除法表达（`context_size / 50`）。
pub fn injection_token_budget(context_size: usize) -> usize {
    context_size / 50
}

/// Compact 保留 tail（recent messages）的 token 预算封顶：context window
/// 的 3%（#1773 由 5% 收紧，尾部只保留最近上下文）。与 L1
/// `scaled_for_context_window` 同路子——大窗口允许更大 tail 预算，
/// 小窗口自动收紧；条数 10% 候选超过该预算时向内收缩。
pub fn compact_tail_token_cap(context_size: usize) -> usize {
    context_size * COMPACT_TAIL_WINDOW_PERCENT / 100
}

/// map-reduce 分块摘要的单块目标 token 数（#1486）。
///
/// 按上下文总长度比例切（context_size / 8），大窗口模型允许更大的块，
/// 小窗口模型自动收紧；带上下限保护：
/// - 上限 40k：保证单块摘要请求（COMPACT_PROMPT + chunk + previous_summary）
///   不会超出常见 provider 的输入限制；
/// - 下限 8k：块太小没有分块意义。
pub fn compact_chunk_target_tokens(context_size: usize) -> usize {
    (context_size / 8).clamp(8_000, 40_000)
}

/// Calculate the effective context window size (after reserving output tokens
/// and summary budget). Output reservation is clamped to at most 25% of the
/// window (#1626) so the effective window never collapses to zero.
pub fn effective_context_window(context_size: usize, max_output_tokens: usize) -> usize {
    let reserved =
        summary_budget(context_size) + clamped_max_output(context_size, max_output_tokens);
    context_size.saturating_sub(reserved)
}

/// Calculate the autocompact trigger threshold.
/// Formula: effective_context_window * ratio
///
/// `ratio` 为触发阈值占 effective window 的比例（默认路径 0.8，可由
/// `context.auto_compact_threshold_ratio` 配置）。函数内 clamp 到
/// share 定义的 `[0.5, 0.95]` 安全区间作为单一防线：下限防 compact
/// 风暴（#1626），上限防估算缓冲归零。
pub fn autocompact_threshold(context_size: usize, max_output_tokens: usize, ratio: f64) -> usize {
    let effective = effective_context_window(context_size, max_output_tokens);
    let safe_ratio = ratio.clamp(
        share::config::context::AUTO_COMPACT_THRESHOLD_RATIO_MIN,
        share::config::context::AUTO_COMPACT_THRESHOLD_RATIO_MAX,
    );
    ((effective as f64) * safe_ratio) as usize
}

/// Estimate the token overhead of tool schemas.
/// Tool schemas are JSON objects sent with every API call.
/// This is a significant fixed cost that must be accounted for.
pub fn estimate_tool_schemas_tokens(tool_schemas: &[tools::ToolDefinition]) -> usize {
    tool_schemas
        .iter()
        .map(|schema| estimate_json_tokens(&schema_json(schema)))
        .sum()
}

/// 工具定义的估算用 JSON 形态（序列化仅发生在估算内部）。
fn schema_json(schema: &tools::ToolDefinition) -> String {
    serde_json::to_string(schema).unwrap_or_default()
}

#[cfg(test)]
#[path = "token_budget_tests.rs"]
mod token_budget_tests;
