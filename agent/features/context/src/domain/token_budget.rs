//! Token estimation service for context management
//!
//! Provides CJK-aware token estimation for messages and text content.
//! Note: This uses estimation algorithms, not actual tokenizers.
//! For more accurate results, consider integrating tiktoken.

use share::message::{ContentBlock, Message};

// ── 预算与估算函数 ──────────────────────────────────────────────
// （历史遗留的 TokenEstimation / ContextUsage 包装类型无任何消费者，
// 已在 #1486 修复中删除；以下为仍在使用的纯函数。）

/// Estimate token count for a string.
/// Uses CJK-aware estimation: CJK characters average ~1 token each
/// (mainstream tokenizers measure 0.7–1.0 tokens per CJK char),
/// while ASCII/Latin text averages ~4 characters per token.
pub fn estimate_tokens(text: &str) -> usize {
    estimate_tokens_with_ratio(text, 4.0)
}

/// Estimate tokens with custom bytes-per-token ratio
pub fn estimate_tokens_with_ratio(text: &str, bytes_per_token: f64) -> usize {
    let mut cjk_chars = 0usize;
    let mut other_bytes = 0usize;

    for ch in text.chars() {
        if is_cjk_char(ch) {
            cjk_chars += 1;
        } else {
            other_bytes += ch.len_utf8();
        }
    }

    // CJK: ~1 token per character; Other: ~N bytes per token (varies by model).
    // 无额外 safety margin：compact threshold 已含 0.8 安全系数，
    // 旧实现（CJK×2 + 1.33x margin）实测高估 1.8–3.2 倍（#1500）。
    let cjk_tokens = cjk_chars;
    let ratio = bytes_per_token.clamp(2.0, 6.0);
    let other_tokens = (other_bytes as f64 / ratio).ceil() as usize;
    cjk_tokens + other_tokens
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

/// Estimate tokens for JSON content (~4 bytes per token, same as text;
/// 旧实现按 2 bytes/token × 1.33 高估 ~2.7 倍，tool schemas 因 JSON 占比高
/// 是 heuristic 判定系统性高估的主要来源之一，#1500)。
pub fn estimate_json_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
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
        })
        .sum::<usize>()
}

// ---- Autocompact threshold constants ----
// effective = context_size - reserved_context(2%) - clamped_max_output(≤25% 窗口)
// threshold = effective * 0.8

/// max_output 预留占窗口的比例上限（#1626）。
///
/// 未设护栏时 `max_output >= 窗口×98%`（如 8k 窗口 + 默认 8192 output）
/// 会让 effective 归零、threshold 归零，任意一轮对话即恒触发
/// auto-compact，形成 compact 风暴直至熔断。预留 clamp 到窗口 25% 后
/// threshold 永不为 0；大窗口常规配置（如 200k 窗口 + 16k output）不受影响。
pub const MAX_OUTPUT_WINDOW_RATIO_CAP: usize = 4;

/// clamp 后 effective window 仍低于此值时判定窗口配置错误（#1626）。
///
/// 低于该值的可用窗口连 system prompt 都无法稳定容纳，auto-compact
/// 只会风暴；此时应禁用 auto-compact 并告警（见 `MisconfiguredWindow`）。
pub const MIN_EFFECTIVE_WINDOW: usize = 1_024;

/// max_output 预留 clamp 到窗口比例上限（#1626 短窗口护栏）。
pub fn clamped_max_output(context_size: usize, max_output_tokens: usize) -> usize {
    max_output_tokens
        .min(context_size / MAX_OUTPUT_WINDOW_RATIO_CAP)
        .max(1)
}

/// Reserved context for guidance and compaction summary.
/// 预留上下文预算：context window 的 2%。
pub fn summary_budget(context_size: usize) -> usize {
    context_size / 50
}

/// Compact 保留 tail（recent messages）的 token 预算封顶：context window
/// 的 5%（#1688）。与 L1 `scaled_for_context_window` 同路子——大窗口允许
/// 更大 tail 预算，小窗口自动收紧；条数 10% 候选超过该预算时向内收缩。
pub fn compact_tail_token_cap(context_size: usize) -> usize {
    context_size / 20
}

/// fallback/护栏中 previous_summary 允许嵌入的最大字符数（#1486）。
///
/// 多次 compact 时 previous_summary 若被全文 verbatim 嵌入会线性累加，
/// 最终撑爆 system prompt（真实事故：92 万字符 summary）。超过此上限时
/// 只保留 previous_summary 的关键尾部（最新状态），头部信息允许丢弃。
/// 定义在 domain 层，供 adapter（compact_summary）与 application
/// （active_summary 注入护栏）共同引用，避免 COLA 分层越界。
pub const FALLBACK_PREVIOUS_SUMMARY_CAP: usize = 20_000;

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
/// Formula: effective_context_window * 0.8
pub fn autocompact_threshold(context_size: usize, max_output_tokens: usize) -> usize {
    let effective = effective_context_window(context_size, max_output_tokens);
    ((effective as f64) * 0.8) as usize
}

/// Estimate the token overhead of tool schemas.
/// Tool schemas are JSON objects sent with every API call.
/// This is a significant fixed cost that must be accounted for.
pub fn estimate_tool_schemas_tokens(tool_schemas: &[serde_json::Value]) -> usize {
    tool_schemas
        .iter()
        .map(|schema| estimate_json_tokens(&schema.to_string()))
        .sum()
}

#[cfg(test)]
#[path = "token_budget_tests.rs"]
mod token_budget_tests;
