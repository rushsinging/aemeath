//! Message compaction utilities
//!
//! Provides message history compression to reduce context usage.
//!
//! ## Context Management Strategy (3 layers)
//!
//! 1. **Tool result truncation** — Per-result size limit. Large results are truncated
//!    with a preview before being added to conversation history.
//! 2. **Microcompact** — Strips old tool result content from early messages.
//! 3. **Full compaction** — LLM-based summarization of early conversation history.

use crate::message::{ContentBlock, Message, Role};

// Re-export token estimation functions for backwards compatibility
pub use crate::token_estimation::{
    autocompact_threshold, compaction_urgency, effective_context_window, estimate_json_tokens,
    estimate_messages_tokens, estimate_tokens, estimate_tool_schemas_tokens, needs_compaction,
    needs_compaction_actual, needs_compaction_full, needs_compaction_with_output,
};

// ---- Tool result size limits ----

/// Maximum characters for a single tool result before truncation.
/// Results exceeding this are truncated with a preview header + tail.
const MAX_TOOL_RESULT_CHARS: usize = 50_000;

/// How many characters to keep as preview from the beginning of a truncated result.
const TRUNCATION_PREVIEW_HEAD: usize = 2_000;

/// How many characters to keep from the end of a truncated result.
const TRUNCATION_PREVIEW_TAIL: usize = 500;

/// Maximum total characters for all tool results in a single message.
/// When exceeded, the largest results are truncated first.
const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;

/// Truncate a single tool result string if it exceeds `MAX_TOOL_RESULT_CHARS`.
/// Returns the (possibly truncated) string.
pub fn truncate_tool_result(output: &str) -> String {
    if output.len() <= MAX_TOOL_RESULT_CHARS {
        return output.to_string();
    }

    let head = safe_slice(output, TRUNCATION_PREVIEW_HEAD);
    let tail = safe_slice_tail(output, TRUNCATION_PREVIEW_TAIL);

    format!(
        "{head}\n\n[... truncated {} chars, showing first {} + last {} chars ...]\n\n{tail}",
        output.len(),
        head.len(),
        tail.len(),
    )
}

/// Apply per-message budget: if total tool result content exceeds the limit,
/// truncate the largest results first until under budget.
pub fn apply_tool_result_budget(message: &mut Message) {
    // Collect sizes of tool result blocks
    let mut result_sizes: Vec<(usize, usize)> = Vec::new(); // (index, size)
    let mut total_chars = 0usize;

    for (i, block) in message.content.iter().enumerate() {
        if let ContentBlock::ToolResult { content, .. } = block {
            let size = match content {
                serde_json::Value::String(s) => s.len(),
                _ => content.to_string().len(),
            };
            result_sizes.push((i, size));
            total_chars += size;
        }
    }

    if total_chars <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
        return;
    }

    // Sort by size descending — truncate largest first
    result_sizes.sort_by(|a, b| b.1.cmp(&a.1));

    for (idx, _size) in result_sizes {
        if total_chars <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
            break;
        }
        if let ContentBlock::ToolResult { ref mut content, .. } = message.content[idx] {
            let old_text = match content {
                serde_json::Value::String(s) => s.clone(),
                _ => content.to_string(),
            };
            if old_text.len() > MAX_TOOL_RESULT_CHARS {
                let truncated = truncate_tool_result(&old_text);
                total_chars -= old_text.len();
                total_chars += truncated.len();
                *content = serde_json::Value::String(truncated);
            }
        }
    }
}

/// Truncate tool results in a list of (id, output, is_error, images) tuples
/// before they are assembled into a Message.
pub fn truncate_tool_results(
    results: &mut Vec<(String, String, bool, Vec<crate::tool::ImageData>)>,
) {
    for (_id, output, _is_error, _images) in results.iter_mut() {
        if output.len() > MAX_TOOL_RESULT_CHARS {
            *output = truncate_tool_result(output);
        }
    }
}

/// Safe UTF-8 slice from the beginning, ensuring we don't split a char boundary.
pub fn safe_slice(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Safe UTF-8 slice from the end.
fn safe_slice_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

// ---- Auto-compact circuit breaker ----

/// Maximum consecutive autocompact failures before the circuit breaker trips.
const MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES: u8 = 3;

/// Tracks autocompact state across turns within a session.
#[derive(Debug, Clone, Default)]
pub struct AutoCompactState {
    /// Number of times compaction has been performed this session.
    pub compaction_count: u32,
    /// Consecutive autocompact failures. Reset on success.
    pub consecutive_failures: u8,
    /// Whether the circuit breaker has tripped (no more retries).
    pub circuit_broken: bool,
}

impl AutoCompactState {
    /// Record a successful compaction — resets the failure counter.
    pub fn record_success(&mut self) {
        self.compaction_count += 1;
        self.consecutive_failures = 0;
        self.circuit_broken = false;
    }

    /// Record a failed compaction — increments the failure counter and
    /// trips the circuit breaker after `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES`.
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES {
            self.circuit_broken = true;
            log::warn!(
                "[autocompact] circuit breaker tripped after {} consecutive failures — skipping future attempts",
                self.consecutive_failures
            );
        }
    }

    /// Returns true if autocompact should be attempted.
    pub fn should_attempt(&self) -> bool {
        !self.circuit_broken
    }
}

/// Microcompact: strip old tool results to save tokens.
/// Clears tool result content for old messages, keeping only recent ones.
pub fn microcompact(messages: &mut Vec<Message>, keep_recent: usize) {
    if messages.len() <= keep_recent {
        return;
    }

    let cutoff = messages.len() - keep_recent;
    for msg in messages[..cutoff].iter_mut() {
        for block in msg.content.iter_mut() {
            if let ContentBlock::ToolResult {
                ref mut content, ..
            } = block
            {
                let content_len = match content {
                    serde_json::Value::String(s) => s.len(),
                    _ => content.to_string().len(),
                };
                if content_len > 100 {
                    *content = serde_json::Value::String(
                        format!("[output truncated, was {} chars]", content_len)
                    );
                }
            }
        }
    }
}

/// Compact messages using local text extraction (fallback when LLM is unavailable).
/// Returns (compacted_messages, was_compacted)
pub fn compact_messages(
    messages: &[Message],
    system_prompt: &str,
    context_size: usize,
) -> (Vec<Message>, bool) {
    if !needs_compaction(messages, system_prompt, context_size) {
        return (messages.to_vec(), false);
    }

    let total = messages.len();
    if total <= 4 {
        return (messages.to_vec(), false);
    }

    // Step 1: Try microcompact first (keep last 10 messages' tool results intact)
    let mut result = messages.to_vec();
    microcompact(&mut result, 10);

    if !needs_compaction(&result, system_prompt, context_size) {
        return (result, true);
    }

    // Step 2: Full compaction — head/tail protection
    // Head: protect first 2 messages (initial conversation turn)
    let head_protect = 2usize.min(total);
    // Tail: keep ~30% of messages as recent context
    let tail_budget = total * 30 / 100;
    let keep_recent = tail_budget.max(4).min(total - head_protect);
    let split_point = total - keep_recent;

    // Never compress into the head-protected zone
    let split_point = split_point.max(head_protect);

    if split_point <= head_protect {
        // Not enough messages to compress
        return (result, false);
    }

    let early_messages = &result[head_protect..split_point];
    let summary = build_summary_text(early_messages);

    // Reassemble: head + summary + recent
    let mut compacted = Vec::with_capacity(head_protect + keep_recent + 3);
    compacted.extend_from_slice(&result[..head_protect]);

    let summary_text = format!(
        "<system-reminder>\n[Conversation summary of {} earlier messages]\n{}\n</system-reminder>",
        early_messages.len(), summary
    );
    compacted.push(Message::user(summary_text));
    compacted.push(Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "I understand. I'll continue from where we left off.".to_string(),
        }],
    });
    compacted.extend_from_slice(&result[split_point..]);

    (compacted, true)
}

// ---- LLM-based compaction ----

/// The prompt template sent to the LLM when compacting conversation history.
const COMPACT_PROMPT: &str = r#"You are a conversation summarizer. Create a structured summary of the conversation.

<instructions>
Produce a summary using the EXACT structure below inside `<summary>` tags.

## Goal
The user's ultimate objective (one sentence).

## Progress
What has been accomplished so far. Include specific file paths, function names, and concrete details.

## Key Decisions
Important decisions made and their reasons.

## Relevant Files
List of key files involved (paths only).

## Current State
Where things stand right now — what's working, what's not.

## Next Steps
What needs to happen next to complete the goal.

Rules:
- Be specific: include file paths, function names, variable names.
- Keep concise: aim for 20-30% of original content length.
- Do NOT include raw tool output or tool call details — focus on semantic meaning.
- Each section can be empty if not applicable, but include the heading.
</instructions>

Here is the conversation to summarize:
"#;

/// Build the LLM compaction request messages from early conversation history.
pub fn build_compact_request(early_messages: &[Message]) -> Vec<Message> {
    let mut conversation_text = String::new();
    for msg in early_messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };

        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    conversation_text.push_str(&format!("[{role}]: {text}\n\n"));
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    // Include tool name and a truncated input summary
                    let input_str = input.to_string();
                    let truncated = if input_str.len() > 500 {
                        format!("{}...", safe_slice(&input_str, 500))
                    } else {
                        input_str
                    };
                    conversation_text
                        .push_str(&format!("[{role} calls {name}]: {truncated}\n\n"));
                }
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    let label = if *is_error { "error" } else { "result" };
                    let content_str = match content {
                        serde_json::Value::String(s) => s.clone(),
                        _ => content.to_string(),
                    };
                    let truncated = if content_str.len() > 1000 {
                        format!("{}...", safe_slice(&content_str, 1000))
                    } else {
                        content_str
                    };
                    conversation_text.push_str(&format!("[tool {label}]: {truncated}\n\n"));
                }
                ContentBlock::Image { .. } => {
                    conversation_text.push_str(&format!("[{role}]: [image]\n\n"));
                }
                ContentBlock::Thinking { .. } => {
                    // Thinking blocks are internal, skip in compaction summary
                }
            }
        }
    }

    let prompt = format!("{COMPACT_PROMPT}\n<conversation>\n{conversation_text}</conversation>\n\nWrite your summary inside <summary> tags.");

    vec![Message::user(prompt)]
}

/// Parse the LLM's compaction response to extract the summary text.
pub fn parse_compact_response(response_text: &str) -> String {
    // Extract content between <summary> tags
    if let Some(start) = response_text.find("<summary>") {
        if let Some(end) = response_text.find("</summary>") {
            let start = start + "<summary>".len();
            if start < end {
                return response_text[start..end].trim().to_string();
            }
        }
    }
    // Fallback: use the entire response
    response_text.trim().to_string()
}

// ---- Post-compact file restoration ----

/// Maximum number of recently-read files to restore after compaction.
const POST_COMPACT_MAX_FILES: usize = 5;

/// Maximum tokens per restored file.
const POST_COMPACT_MAX_TOKENS_PER_FILE: usize = 5_000;

/// Total token budget for all restored files.
const POST_COMPACT_TOKEN_BUDGET: usize = 50_000;

/// Build file restoration attachments from the set of recently-read file paths.
/// Reads the most recently modified files (up to budget) and returns a summary
/// message to inject after compaction.
pub fn build_file_restoration(read_files: &std::collections::HashSet<String>) -> Option<String> {
    if read_files.is_empty() {
        return None;
    }

    // Collect files with their modification times, sorted by recency
    let mut files_with_mtime: Vec<(String, std::time::SystemTime)> = read_files
        .iter()
        .filter_map(|path| {
            let metadata = std::fs::metadata(path).ok()?;
            let mtime = metadata.modified().ok()?;
            Some((path.clone(), mtime))
        })
        .collect();

    files_with_mtime.sort_by(|a, b| b.1.cmp(&a.1)); // Most recent first

    let mut restored_content = String::new();
    let mut total_tokens = 0usize;
    let mut file_count = 0usize;

    for (path, _mtime) in files_with_mtime.iter().take(POST_COMPACT_MAX_FILES) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_tokens = estimate_tokens(&content);
        let truncated = if file_tokens > POST_COMPACT_MAX_TOKENS_PER_FILE {
            // Truncate to approximate token budget
            let max_chars = POST_COMPACT_MAX_TOKENS_PER_FILE * 4; // ~4 chars/token
            let end = max_chars.min(content.len());
            let mut boundary = end;
            while boundary > 0 && !content.is_char_boundary(boundary) {
                boundary -= 1;
            }
            format!("{}...\n[truncated, {} total chars]", &content[..boundary], content.len())
        } else {
            content
        };

        let entry_tokens = estimate_tokens(&truncated) + 20; // overhead for tags
        if total_tokens + entry_tokens > POST_COMPACT_TOKEN_BUDGET {
            break;
        }

        restored_content.push_str(&format!(
            "\n<file path=\"{path}\">\n{truncated}\n</file>\n"
        ));
        total_tokens += entry_tokens;
        file_count += 1;
    }

    if file_count == 0 {
        return None;
    }

    Some(format!(
        "<system-reminder>\n[Post-compaction file restoration: {} recently-read files]\n{restored_content}\n</system-reminder>",
        file_count
    ))
}

/// Fix orphaned tool-use / tool-result pairs after compaction.
pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
    let mut tool_use_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tool_result_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for msg in messages.iter() {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    tool_use_ids.insert(id.clone());
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    tool_result_ids.insert(tool_use_id.clone());
                }
                _ => {}
            }
        }
    }

    // Remove orphan ToolResults (no matching ToolUse)
    let orphan_results: std::collections::HashSet<&String> =
        tool_result_ids.difference(&tool_use_ids).collect();
    if !orphan_results.is_empty() {
        for msg in messages.iter_mut() {
            msg.content.retain(|block| {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    !orphan_results.contains(tool_use_id)
                } else {
                    true
                }
            });
        }
    }

    // Add placeholder results for ToolUse blocks without results
    let missing_results: Vec<String> = tool_use_ids
        .difference(&tool_result_ids)
        .cloned()
        .collect();
    if !missing_results.is_empty() {
        let placeholder_msg = Message {
            role: Role::User,
            content: missing_results
                .into_iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: serde_json::json!("[result removed during compaction]"),
                    is_error: false,
                })
                .collect(),
        };
        let insert_pos = if messages.is_empty() { 0 } else { messages.len() - 1 };
        messages.insert(insert_pos, placeholder_msg);
    }
}

/// Assemble final compacted messages from a summary + recent messages.
pub fn assemble_compacted(
    summary: String,
    recent_messages: &[Message],
    original_early_count: usize,
) -> (Vec<Message>, bool) {
    assemble_compacted_with_files(summary, recent_messages, original_early_count, None)
}

/// Assemble compacted messages with optional file restoration.
pub fn assemble_compacted_with_files(
    summary: String,
    recent_messages: &[Message],
    original_early_count: usize,
    read_files: Option<&std::collections::HashSet<String>>,
) -> (Vec<Message>, bool) {
    let mut compacted = Vec::with_capacity(recent_messages.len() + 4);

    // Summary message
    let mut summary_text = format!(
        "<system-reminder>\n[Conversation summary of {} earlier messages]\n{}\n</system-reminder>",
        original_early_count, summary
    );

    // Append file restoration if available
    if let Some(files) = read_files {
        if let Some(restoration) = build_file_restoration(files) {
            summary_text.push_str("\n\n");
            summary_text.push_str(&restoration);
        }
    }

    compacted.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: summary_text,
        }],
    });

    compacted.push(Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "Understood. I have the context from our earlier conversation. Let me continue."
                .to_string(),
        }],
    });

    for msg in recent_messages {
        compacted.push(msg.clone());
    }

    fix_role_alternation(&mut compacted);
    sanitize_tool_pairs(&mut compacted);
    (compacted, true)
}

/// Build a local text summary from early messages (fallback, no LLM call).
fn build_summary_text(messages: &[Message]) -> String {
    let mut summary = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        let text = msg.text_content();
        if !text.is_empty() {
            let truncated = if text.len() > 200 {
                format!("{}...", safe_slice(&text, 200))
            } else {
                text
            };
            summary.push_str(&format!("- {role}: {truncated}\n"));
        }

        // Note tool usage without full details
        let tool_uses = msg.extract_tool_uses();
        if !tool_uses.is_empty() {
            let tool_names: Vec<&str> = tool_uses.iter().map(|(_, name, _)| *name).collect();
            summary.push_str(&format!(
                "- {role} used tools: {}\n",
                tool_names.join(", ")
            ));
        }
    }
    summary
}

/// Ensure messages alternate between User and Assistant roles
fn fix_role_alternation(messages: &mut Vec<Message>) {
    let mut i = 1;
    while i < messages.len() {
        if messages[i].role == messages[i - 1].role {
            let filler_role = match messages[i].role {
                Role::User => Role::Assistant,
                Role::Assistant => Role::User,
            };
            let filler = Message {
                role: filler_role,
                content: vec![ContentBlock::Text {
                    text: "(continued)".to_string(),
                }],
            };
            messages.insert(i, filler);
            i += 1;
        }
        i += 1;
    }
}
