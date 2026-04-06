//! Message compaction utilities
//!
//! Provides message history compression to reduce context usage.

use crate::message::{ContentBlock, Message, Role};

// Re-export token estimation functions for backwards compatibility
pub use crate::token_estimation::{
    estimate_json_tokens, estimate_messages_tokens, estimate_tokens, needs_compaction,
};

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

    // Step 2: Full compaction — summarize early messages
    let keep_recent = (total * 40 / 100).max(4).min(total - 1);
    let split_point = total - keep_recent;

    let early_messages = &result[..split_point];
    let summary = build_summary_text(early_messages);

    assemble_compacted(summary, &result[split_point..], split_point)
}

// ---- LLM-based compaction ----

/// The prompt template sent to the LLM when compacting conversation history.
const COMPACT_PROMPT: &str = r#"You are a conversation summarizer. Your task is to create a detailed summary of the conversation so far.

<instructions>
1. Analyze the conversation carefully and produce a summary in `<summary>` tags.
2. The summary should capture:
   - The user's original goal and requirements
   - Key decisions made during the conversation
   - What has been accomplished so far (files created/modified, commands run, etc.)
   - Current state and any pending work
   - Important context that would be lost without this summary
3. Be specific: include file paths, function names, variable names, and other concrete details.
4. Do NOT include tool call details or raw output — focus on the semantic meaning.
5. Keep the summary concise but comprehensive. Aim for roughly 20-30% of the original content length.
6. Write the summary as a narrative, not a list.
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
                        format!("{}...", &input_str[..500])
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
                        format!("{}...", &content_str[..1000])
                    } else {
                        content_str
                    };
                    conversation_text.push_str(&format!("[tool {label}]: {truncated}\n\n"));
                }
                ContentBlock::Image { .. } => {
                    conversation_text.push_str(&format!("[{role}]: [image]\n\n"));
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

/// Assemble final compacted messages from a summary + recent messages.
pub fn assemble_compacted(
    summary: String,
    recent_messages: &[Message],
    original_early_count: usize,
) -> (Vec<Message>, bool) {
    let mut compacted = Vec::with_capacity(recent_messages.len() + 2);

    compacted.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: format!(
                "<system-reminder>\n[Conversation summary of {} earlier messages]\n{}\n</system-reminder>",
                original_early_count, summary
            ),
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
                format!("{}...", &text[..200])
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
