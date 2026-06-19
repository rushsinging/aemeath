//! Token estimation service for context management
//!
//! Provides CJK-aware token estimation for messages and text content.
//! Note: This uses estimation algorithms, not actual tokenizers.
//! For more accurate results, consider integrating tiktoken.

use share::message::{ContentBlock, Message};

/// Token estimation service
pub struct TokenEstimation {
    /// Context size limit
    context_size: usize,
    /// Warning threshold (percentage)
    warning_threshold: u8,
    /// Bytes per token ratio (default 4, varies by model)
    bytes_per_token: f64,
}

impl TokenEstimation {
    /// Create a new token estimation service
    pub fn new(context_size: usize) -> Self {
        Self {
            context_size,
            warning_threshold: 80,
            bytes_per_token: 4.0, // Default for most models
        }
    }

    /// Set warning threshold (percentage of context used)
    pub fn with_warning_threshold(mut self, threshold: u8) -> Self {
        self.warning_threshold = threshold.min(100);
        self
    }

    /// Set bytes per token ratio based on model
    /// Some models use different ratios (e.g., 3.5 for efficient models)
    pub fn with_model_ratio(mut self, bytes_per_token: f64) -> Self {
        self.bytes_per_token = bytes_per_token.clamp(2.0, 6.0);
        self
    }

    /// Estimate tokens for text content
    pub fn estimate_text(&self, text: &str) -> usize {
        estimate_tokens(text)
    }

    /// Estimate tokens for a message
    pub fn estimate_message(&self, message: &Message) -> usize {
        estimate_message_tokens(message)
    }

    /// Estimate tokens for a list of messages
    pub fn estimate_messages(&self, messages: &[Message]) -> usize {
        estimate_messages_tokens(messages)
    }

    /// Estimate tokens for JSON content
    pub fn estimate_json(&self, json: &str) -> usize {
        estimate_json_tokens(json)
    }

    /// Check if messages exceed warning threshold
    pub fn is_near_limit(&self, messages: &[Message], system_prompt: &str) -> bool {
        let total = self.total_tokens(messages, system_prompt);
        total > self.context_size * self.warning_threshold as usize / 100
    }

    /// Get total tokens including system prompt
    pub fn total_tokens(&self, messages: &[Message], system_prompt: &str) -> usize {
        let system_tokens = self.estimate_text(system_prompt);
        let message_tokens = self.estimate_messages(messages);
        system_tokens + message_tokens
    }

    /// Get context usage statistics
    pub fn usage_stats(&self, messages: &[Message], system_prompt: &str) -> ContextUsage {
        let system_tokens = self.estimate_text(system_prompt);
        let message_tokens = self.estimate_messages(messages);
        let total = system_tokens + message_tokens;
        let available = self.context_size.saturating_sub(total);
        let percentage = (total as f64 / self.context_size as f64 * 100.0) as u8;

        ContextUsage {
            total_tokens: total,
            system_tokens,
            message_tokens,
            context_size: self.context_size,
            available_tokens: available,
            usage_percentage: percentage,
            needs_compaction: percentage >= self.warning_threshold,
        }
    }
}

impl Default for TokenEstimation {
    fn default() -> Self {
        Self::new(128000) // Default context size
    }
}

/// Context usage statistics
#[derive(Debug, Clone)]
pub struct ContextUsage {
    /// Total tokens used
    pub total_tokens: usize,
    /// Tokens in system prompt
    pub system_tokens: usize,
    /// Tokens in messages
    pub message_tokens: usize,
    /// Maximum context size
    pub context_size: usize,
    /// Available tokens remaining
    pub available_tokens: usize,
    /// Usage percentage
    pub usage_percentage: u8,
    /// Whether compaction is needed
    pub needs_compaction: bool,
}

impl ContextUsage {
    /// Format as human-readable string
    pub fn format(&self) -> String {
        let status = if self.needs_compaction {
            "⚠️ Near limit"
        } else {
            "✓ OK"
        };

        format!(
            "Context Usage: {} / {} tokens ({}%)\n  System: {} tokens\n  Messages: {} tokens\n  Available: {} tokens\n  Status: {}",
            format_tokens(self.total_tokens),
            format_tokens(self.context_size),
            self.usage_percentage,
            format_tokens(self.system_tokens),
            format_tokens(self.message_tokens),
            format_tokens(self.available_tokens),
            status
        )
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
            ContentBlock::Thinking { thinking } => estimate_tokens(thinking),
        })
        .sum::<usize>()
}

// ---- Autocompact threshold constants ----
// Following Claude Code TS's formula:
// effective_window = context_window - reserved_output
// threshold = effective_window - buffer

/// Reserved tokens for compaction summary output (p99.99 ≈ 17.4K, use 20K).
const MAX_OUTPUT_TOKENS_FOR_SUMMARY: usize = 20_000;

/// Safety buffer below the effective window before triggering compaction.
const AUTOCOMPACT_BUFFER_TOKENS: usize = 13_000;

/// Calculate the effective context window size (after reserving output tokens).
pub fn effective_context_window(context_size: usize, max_output_tokens: usize) -> usize {
    let reserved = max_output_tokens.min(MAX_OUTPUT_TOKENS_FOR_SUMMARY);
    context_size.saturating_sub(reserved)
}

/// Calculate the autocompact trigger threshold.
/// Formula: (context_size - min(max_output, 20K)) - 13K
pub fn autocompact_threshold(context_size: usize, max_output_tokens: usize) -> usize {
    effective_context_window(context_size, max_output_tokens)
        .saturating_sub(AUTOCOMPACT_BUFFER_TOKENS)
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

/// Check if messages need compaction given a context size limit (in tokens).
/// Uses the TS-style threshold formula that reserves output + buffer tokens.
/// Includes a fixed overhead estimate for tool schemas (~15K tokens for 25 tools).
pub fn needs_compaction(messages: &[Message], system_prompt: &str, context_size: usize) -> bool {
    needs_compaction_full(messages, system_prompt, context_size, 0)
}

/// Check compaction with explicit tool schema token count.
pub fn needs_compaction_full(
    messages: &[Message],
    system_prompt: &str,
    context_size: usize,
    tool_schema_tokens: usize,
) -> bool {
    let system_tokens = estimate_tokens(system_prompt);
    let message_tokens = estimate_messages_tokens(messages);
    let total = system_tokens + message_tokens + tool_schema_tokens;
    total > autocompact_threshold(context_size, 8192)
}

/// Check if messages need compaction with explicit max_output_tokens.
pub fn needs_compaction_with_output(
    messages: &[Message],
    system_prompt: &str,
    context_size: usize,
    max_output_tokens: usize,
) -> bool {
    let system_tokens = estimate_tokens(system_prompt);
    let message_tokens = estimate_messages_tokens(messages);
    let total = system_tokens + message_tokens;
    total > autocompact_threshold(context_size, max_output_tokens)
}

/// Check if compaction is needed using actual API-reported token count.
///
/// - `last_input_tokens`: Total input tokens reported by the API (includes cached tokens).
/// - `last_output_tokens`: Total output tokens reported by the API (includes reasoning tokens).
/// - `cached_tokens`: Tokens served from prompt cache (still consume context, but cost less/free).
/// - `reasoning_tokens`: Tokens consumed by reasoning/thinking (consume context).
/// - `context_size`: The model's context window size.
pub fn needs_compaction_actual(
    last_input_tokens: u64,
    last_output_tokens: u64,
    _cached_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    context_size: usize,
) -> bool {
    let reasoning = reasoning_tokens.unwrap_or(0);

    // All input tokens (including cached) consume context window
    // Reasoning tokens are extra context consumption
    let total = last_input_tokens + last_output_tokens + reasoning;

    let threshold = autocompact_threshold(context_size, 8192) as u64;
    total > threshold
}

/// Determine the compaction urgency level based on actual token usage.
/// Uses effective_context_window for percentage calculation.
/// Returns a level from 0-3:
/// - 0: No compaction needed (< 70% of effective window)
/// - 1: Approaching limit, microcompact recommended (70-80%)
/// - 2: At limit, full compaction needed (80-90%)
/// - 3: Critical, blocking — must compact before next query (> 90%)
///
/// - `last_input_tokens`: Total input tokens reported by the API (includes cached tokens).
/// - `cached_tokens`: Tokens served from prompt cache (still consume context, but cost less/free).
/// - `reasoning_tokens`: Tokens consumed by reasoning/thinking (consume context).
/// - `context_size`: The model's context window size.
pub fn compaction_urgency(
    last_input_tokens: u64,
    _cached_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    context_size: usize,
) -> u8 {
    let reasoning = reasoning_tokens.unwrap_or(0);

    // All input tokens (including cached) consume context window
    let total = last_input_tokens + reasoning;

    let effective = effective_context_window(context_size, 8192) as u64;
    let pct = total * 100 / effective.max(1);
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
