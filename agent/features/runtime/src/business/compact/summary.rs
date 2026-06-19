//! 消息压缩 — 本地文本摘要和 LLM 摘要
//!
//! 提供 `compact_messages` 作为本地压缩入口，以及 LLM 压缩相关的
//! 请求构建 / 响应解析 / 摘要文本生成。

use crate::business::compact::micro::microcompact;
use crate::business::compact::restore::{fix_role_alternation, sanitize_tool_pairs};
use share::message::{ContentBlock, Message, Role};
use share::string_idx::slice_head;
use tokio_util::sync::CancellationToken;

// 向后兼容的 re-export
pub use crate::business::compact::needs_compaction;

/// 发送给 LLM 的压缩提示模板。
pub const COMPACT_PROMPT: &str = r#"You are a conversation summarizer. Create a structured summary of the conversation.

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

/// 使用本地文本提取压缩消息（LLM 不可用时的回退方案）。
/// 返回 (压缩后的消息, 是否进行了压缩)
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

    // 第一步：先尝试微压缩（保留最近 10 条消息的工具结果）
    let result = microcompact(messages, 10);

    if !needs_compaction(&result, system_prompt, context_size) {
        return (result, true);
    }

    // 第二步：完整压缩 — 头部/尾部保护
    let Some(window) = compact_window(total) else {
        return (result, false);
    };

    let early_messages = &result[window.head_protect..window.split_point];
    let summary = build_summary_text(early_messages);

    // 重组：头部 + 摘要 + 近期消息
    let mut compacted = Vec::with_capacity(window.head_protect + window.keep_recent + 3);
    compacted.extend_from_slice(&result[..window.head_protect]);

    let summary_text = format!(
        "<system-reminder>\n[Conversation summary of {} earlier messages]\n{}\n</system-reminder>",
        early_messages.len(),
        summary
    );
    compacted.push(Message::system_generated_user(summary_text));
    compacted.push(Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "I understand. I'll continue from where we left off.".to_string(),
        }],
        metadata: None,
    });
    compacted.extend_from_slice(&result[window.split_point..]);

    fix_role_alternation(&mut compacted);
    sanitize_tool_pairs(&mut compacted);

    (compacted, true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactWindow {
    pub head_protect: usize,
    pub split_point: usize,
    pub keep_recent: usize,
}

pub fn compact_window(total: usize) -> Option<CompactWindow> {
    if total <= 4 {
        return None;
    }
    let head_protect = 2usize.min(total);
    let tail_budget = total * 30 / 100;
    let keep_recent = tail_budget.max(4).min(total - head_protect);
    let split_point = (total - keep_recent).max(head_protect);
    if split_point <= head_protect {
        return None;
    }
    Some(CompactWindow {
        head_protect,
        split_point,
        keep_recent,
    })
}

pub fn messages_selected_for_precompact_memory(messages: &[Message]) -> Vec<Message> {
    compact_window(messages.len())
        .map(|window| messages[window.head_protect..window.split_point].to_vec())
        .unwrap_or_default()
}

/// 从早期对话历史构建 LLM 压缩请求消息。
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
                    let input_str = input.to_string();
                    let truncated = if input_str.len() > 500 {
                        format!("{}...", slice_head(&input_str, 500))
                    } else {
                        input_str
                    };
                    conversation_text.push_str(&format!("[{role} calls {name}]: {truncated}\n\n"));
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
                        format!("{}...", slice_head(&content_str, 1000))
                    } else {
                        content_str
                    };
                    conversation_text.push_str(&format!("[tool {label}]: {truncated}\n\n"));
                }
                ContentBlock::Image { .. } => {
                    conversation_text.push_str(&format!("[{role}]: [image]\n\n"));
                }
                ContentBlock::Thinking { .. } => {
                    // 思考块是内部的，在压缩摘要中跳过
                }
            }
        }
    }

    let prompt = format!("{COMPACT_PROMPT}\n<conversation>\n{conversation_text}</conversation>\n\nWrite your summary inside <summary> tags.");

    vec![Message::user(prompt)]
}

/// 解析 LLM 的压缩响应，提取摘要文本。
pub fn parse_compact_response(response_text: &str) -> String {
    // 提取 <summary> 标签之间的内容
    if let Some(start) = response_text.find("<summary>") {
        if let Some(end) = response_text.find("</summary>") {
            let start = start + "<summary>".len();
            if start < end {
                return response_text[start..end].trim().to_string();
            }
        }
    }
    // 回退：使用整个响应
    response_text.trim().to_string()
}

/// 从早期消息构建本地文本摘要（回退方案，无 LLM 调用）。
pub fn build_summary_text(messages: &[Message]) -> String {
    let mut summary = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        let text = msg.text_content();
        if !text.is_empty() {
            let truncated = if text.len() > 200 {
                format!("{}...", slice_head(&text, 200))
            } else {
                text
            };
            summary.push_str(&format!("- {role}: {truncated}\n"));
        }

        // 记录工具使用但不包含完整详情
        let tool_uses = msg.extract_tool_uses();
        if !tool_uses.is_empty() {
            let tool_names: Vec<&str> = tool_uses.iter().map(|(_, name, _)| *name).collect();
            summary.push_str(&format!("- {role} used tools: {}\n", tool_names.join(", ")));
        }
    }
    summary
}

/// 使用 LLM 进行语义化压缩（对早期消息生成结构化摘要）。
///
/// 如果 LLM 调用失败，回退到本地 `build_summary_text`。
/// 返回 (压缩后的消息, 是否进行了压缩)
pub async fn compact_messages_with_llm(
    messages: &[Message],
    system_prompt: &str,
    context_size: usize,
    client: Option<&provider::api::LlmClient>,
) -> (Vec<Message>, bool) {
    if !needs_compaction(messages, system_prompt, context_size) {
        return (messages.to_vec(), false);
    }

    let total = messages.len();
    if total <= 4 {
        return (messages.to_vec(), false);
    }

    // 第一步：微压缩（返回克隆视图，不修改原始 messages）
    let result = microcompact(messages, 10);

    if !needs_compaction(&result, system_prompt, context_size) {
        return (result, true);
    }

    // 第二步：完整压缩 — 头部/尾部保护
    let Some(window) = compact_window(total) else {
        return (result, false);
    };

    let early_messages = &result[window.head_protect..window.split_point];

    // 尝试 LLM 摘要，失败则回退到本地
    let summary = match client {
        Some(client) => match llm_compact(client, early_messages).await {
            Ok(text) => text,
            Err(_) => build_summary_text(early_messages),
        },
        None => build_summary_text(early_messages),
    };

    // 重组：头部 + 摘要 + 近期消息
    let mut compacted = Vec::with_capacity(window.head_protect + window.keep_recent + 3);
    compacted.extend_from_slice(&result[..window.head_protect]);

    let summary_text = format!(
        "<system-reminder>\n[Conversation summary of {} earlier messages]\n{}\n</system-reminder>",
        early_messages.len(),
        summary
    );
    compacted.push(Message::system_generated_user(summary_text));
    compacted.push(Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "I understand. I'll continue from where we left off.".to_string(),
        }],
        metadata: None,
    });
    compacted.extend_from_slice(&result[window.split_point..]);

    fix_role_alternation(&mut compacted);
    sanitize_tool_pairs(&mut compacted);

    (compacted, true)
}

/// 调用 LLM 生成语义化压缩摘要
async fn llm_compact(
    client: &provider::api::LlmClient,
    early_messages: &[Message],
) -> Result<String, String> {
    let mut request = build_compact_request(early_messages);
    // 将 COMPACT_PROMPT 作为最后一条 user 消息
    request.push(Message::user(format!(
        "{}\n\n这里是要总结的对话：",
        COMPACT_PROMPT
    )));

    let cancel = CancellationToken::new();
    let collected = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let collected_clone = collected.clone();
    {
        let mut handler = provider::api::CallbackHandler::new(Box::new(move |text: &str| {
            if let Ok(mut guard) = collected_clone.lock() {
                guard.push_str(text);
            }
        }));

        client
            .stream_message(&[], &request, &[], &mut handler, &cancel)
            .await
            .map_err(|e| format!("LLM compact call failed: {e}"))?;
    }

    let full_text = collected.lock().map_err(|e| format!("Lock error: {e}"))?;
    let summary = parse_compact_response(full_text.as_str());
    if summary.is_empty() {
        return Err("LLM returned empty summary".into());
    }
    Ok(summary)
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod summary_tests;
