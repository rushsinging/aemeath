//! Token estimation service for context management
//!
//! Provides CJK-aware token estimation for messages and text content.
//! Note: This uses estimation algorithms, not actual tokenizers.
//! For more accurate results, consider integrating tiktoken.

use share::message::{ContentBlock, Message};

/// Token budget configuration used by every compaction decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudgetConfig {
    context_size: usize,
    max_output_tokens: usize,
    max_summary_output_tokens: usize,
    autocompact_buffer_tokens: usize,
}

impl TokenBudgetConfig {
    /// Build a budget from the resolved model context window and output limit.
    pub const fn new(context_size: usize, max_output_tokens: usize) -> Self {
        Self {
            context_size,
            max_output_tokens,
            max_summary_output_tokens: 20_000,
            autocompact_buffer_tokens: 13_000,
        }
    }

    /// Resolved model context window.
    pub const fn context_size(self) -> usize {
        self.context_size
    }

    /// Resolved model output limit.
    pub const fn max_output_tokens(self) -> usize {
        self.max_output_tokens
    }

    /// Context window remaining after reserving summary output tokens.
    pub fn effective_context_window(self) -> usize {
        let reserved = self.max_output_tokens.min(self.max_summary_output_tokens);
        self.context_size.saturating_sub(reserved)
    }

    /// Auto-compact threshold including the existing 0.8 safety factor.
    pub fn autocompact_threshold(self) -> usize {
        let raw = self
            .effective_context_window()
            .saturating_sub(self.autocompact_buffer_tokens);
        ((raw as f64) * 0.8) as usize
    }
}

/// Format token count with k/m suffix
pub fn format_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        if m.fract() < 0.05 {
            format!("{:.0}m", m)
        } else {
            format!("{:.1}m", m)
        }
    } else if n >= 1000 {
        let k = n as f64 / 1000.0;
        if k.fract() < 0.05 {
            format!("{:.0}k", k)
        } else {
            format!("{:.1}k", k)
        }
    } else {
        n.to_string()
    }
}

/// Estimate token count for a string.
/// Uses CJK-aware estimation: CJK characters average ~2 tokens each,
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

    // CJK: ~2 tokens per character; Other: ~N bytes per token (varies by model)
    // Apply conservative safety margin
    let cjk_tokens = cjk_chars * 2;
    let ratio = bytes_per_token.clamp(2.0, 6.0);
    let other_tokens = (other_bytes as f64 / ratio).ceil() as usize;
    let safety_margin = 4.0 / 3.0; // 1.33x safety margin
    ((cjk_tokens + other_tokens) as f64 * safety_margin).ceil() as usize
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

/// Estimate tokens for JSON content (more dense, ~2 chars per token)
pub fn estimate_json_tokens(text: &str) -> usize {
    let base = text.len().div_ceil(2);
    base * 4 / 3
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

/// Estimate the token overhead of tool schemas.
/// Tool schemas are JSON objects sent with every API call.
/// This is a significant fixed cost that must be accounted for.
pub fn estimate_tool_schemas_tokens(tool_schemas: &[serde_json::Value]) -> usize {
    tool_schemas
        .iter()
        .map(|s| estimate_json_tokens(&s.to_string()))
        .sum()
}

/// Check compaction using an estimated token count.
pub fn needs_compaction(
    messages: &[Message],
    system_prompt: &str,
    config: &TokenBudgetConfig,
) -> bool {
    needs_compaction_full(messages, system_prompt, 0, config)
}

/// Check compaction with explicit tool schema token count.
pub fn needs_compaction_full(
    messages: &[Message],
    system_prompt: &str,
    tool_schema_tokens: usize,
    config: &TokenBudgetConfig,
) -> bool {
    let system_tokens = estimate_tokens(system_prompt);
    let message_tokens = estimate_messages_tokens(messages);
    let total = system_tokens + message_tokens + tool_schema_tokens;
    total > config.autocompact_threshold()
}

/// Check if compaction is needed using actual API-reported token count.
///
/// `last_output_tokens` already includes reasoning tokens. Cached tokens remain part of
/// `last_input_tokens`, so neither value needs a separate adjustment here.
pub fn needs_compaction_actual(
    last_input_tokens: u64,
    last_output_tokens: u64,
    config: &TokenBudgetConfig,
) -> bool {
    let total = last_input_tokens + last_output_tokens;
    total > config.autocompact_threshold() as u64
}

/// Determine the compaction urgency level based on actual token usage.
/// Returns 0 below 70%, 1 at 70-79%, 2 at 80-89%, and 3 at 90%+.
pub fn compaction_urgency(last_input_tokens: u64, config: &TokenBudgetConfig) -> u8 {
    let effective = config.effective_context_window() as u64;
    let pct = last_input_tokens * 100 / effective.max(1);
    match pct {
        0..=69 => 0,
        70..=79 => 1,
        80..=89 => 2,
        _ => 3,
    }
}

#[cfg(test)]
#[path = "token_estimation_tests.rs"]
mod token_estimation_tests;
