//! 消息压缩 — 本地文本摘要和 LLM 摘要
//!
//! 提供 `compact_messages` 作为本地压缩入口，以及 LLM 压缩相关的
//! 请求构建 / 响应解析 / 摘要文本生成。

use crate::domain::compact::{sanitize_tool_pairs, CompactStage};
use share::message::{ContentBlock, Message, Role};
use share::string_idx::slice_head;
use tokio_util::sync::CancellationToken;

/// 将 recent_messages 中所有 ToolResult 文本替换为占位符。
/// recent tail 的工具结果内容已被 summary 涵盖，保留原始大块文本会浪费 context。
/// 保留 tool_use_id 和消息结构（保证 LLM 能继续工具调用链路）。
fn placeholder_tool_results(messages: &mut [Message]) {
    for msg in messages.iter_mut() {
        for block in msg.content.iter_mut() {
            if let ContentBlock::ToolResult {
                text: Some(text),
                tool_use_id,
                ..
            } = block
            {
                if !text.is_empty() {
                    *text = "[tool result omitted during compaction]".to_string();
                    log::debug!(
                        target: "aemeath:agent:storage",
                        "compact placeholder ToolResult {tool_use_id}",
                    );
                }
            }
        }
    }
}

// 向后兼容的 re-export
pub use crate::domain::compact::needs_compaction;

/// Compact 进度回调 trait。
///
/// `compact_messages_with_llm` 在各阶段（Preparing/Summarizing/Finalizing）
/// 调用此回调通知调用方。map-reduce 模式下，每个 chunk 处理前也会调用，
/// 携带 `(current, total)` chunk 计数。
pub trait CompactProgressFn: Send + Sync {
    fn emit(&self, stage: CompactStage, current: Option<usize>, total: Option<usize>);
}

impl<F> CompactProgressFn for F
where
    F: Fn(CompactStage, Option<usize>, Option<usize>) + Send + Sync,
{
    fn emit(&self, stage: CompactStage, current: Option<usize>, total: Option<usize>) {
        self(stage, current, total)
    }
}

/// 发出进度回调的辅助函数（`progress` 为 `None` 时 no-op）。
fn emit_progress(progress: Option<&dyn CompactProgressFn>, stage: CompactStage) {
    if let Some(p) = progress {
        p.emit(stage, None, None);
    }
}

fn emit_progress_chunk(
    progress: Option<&dyn CompactProgressFn>,
    stage: CompactStage,
    current: usize,
    total: usize,
) {
    if let Some(p) = progress {
        p.emit(stage, Some(current), Some(total));
    }
}

/// compact 结果：summary 走 system 通道，recent_messages 作为新链的消息。
#[derive(Debug, Clone)]
pub struct CompactResult {
    /// 早期对话的结构化摘要（拼入 system_blocks）
    pub summary: String,
    /// recent tail（从 split_point 到末尾的原始消息）
    pub recent_messages: Vec<Message>,
}

/// 发送给 LLM 的压缩提示模板。
pub const COMPACT_PROMPT: &str = r#"You are a conversation history compactor for an AI coding agent. Your job is to compress PRIOR conversation history into a structured summary so the agent can continue working with reduced context.

CRITICAL: The text below is PAST conversation history, NOT a new task. Do NOT treat project context files (AGENTS.md, CLAUDE.md, etc.) or environment descriptions as an action request. If the history ends without a clear pending action, summarize what was accomplished — NEVER respond with "please tell me what to do".

Budget: Aim for up to {BUDGET} tokens. This summary replaces the original messages, so it MUST preserve enough detail for the agent to continue seamlessly. More detail is better than less — use the budget fully for long conversations.

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
- Use the full budget (up to {BUDGET} tokens) — more detail helps the agent continue.
- Do NOT include raw tool output or tool call details — focus on semantic meaning.
- Do NOT ask clarifying questions or say "no task found" — this is history compression, not a chat.
- Each section can be empty if not applicable, but include the heading.
</instructions>

Here is the PAST conversation history to compress:
"#;

/// 单个 compact chunk 的目标 token 数。
/// 超过此值的 early_messages 会触发 map-reduce（分块独立摘要 → 合并）。
const COMPACT_CHUNK_TARGET_TOKENS: usize = 30_000;

/// 使用本地文本提取压缩消息（LLM 不可用时的回退方案）。
///
/// 返回 `Some(CompactResult)` 表示发生了压缩（summary + recent tail）；
/// `None` 表示无需压缩。summary 不再注入 messages，走 system 通道。
pub fn compact_messages(
    messages: &[Message],
    system_prompt: &str,
    context_size: usize,
) -> Option<CompactResult> {
    if !needs_compaction(messages, system_prompt, context_size) {
        return None;
    }

    let total = messages.len();
    let window = compact_window(total)?;
    if total <= 4 {
        return None;
    }

    let early_messages = &messages[window.head_protect..window.split_point];
    let summary = build_summary_text(early_messages);

    // recent tail：split_point 到末尾的原始消息
    let mut recent = messages[window.split_point..].to_vec();
    sanitize_tool_pairs(&mut recent);
    // 截断 recent tail 中超阈值的 ToolResult，避免大输出导致 compact 后仍超 context 阈值。
    placeholder_tool_results(&mut recent);

    Some(CompactResult {
        summary,
        recent_messages: recent,
    })
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
    // recent tail 保留尾部 10%（至少 4 条保证工具调用连续性）。
    let tail_budget = total * 10 / 100;
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
pub fn build_compact_request(early_messages: &[Message], context_size: usize) -> Vec<Message> {
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

    let prompt = format!(
        "{COMPACT_PROMPT}\n<conversation_history>\n{conversation_text}</conversation_history>\n\nCompress this history into a summary now. Write your summary inside <summary> tags.",
    )
    .replace(
        "{BUDGET}",
        &crate::domain::token_budget::summary_budget(context_size).to_string(),
    );

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
/// 返回 `Some(CompactResult)` 表示发生了压缩；`None` 表示无需压缩。
/// summary 走 system 通道，不注入 messages。
pub async fn compact_messages_with_llm(
    messages: &[Message],
    system_prompt: &str,
    context_size: usize,
    client: Option<&provider::LlmClient>,
    progress: Option<&dyn CompactProgressFn>,
    cancel: &CancellationToken,
) -> Option<CompactResult> {
    if !needs_compaction(messages, system_prompt, context_size) {
        return None;
    }

    let total = messages.len();
    if total <= 4 {
        return None;
    }

    emit_progress(progress, CompactStage::Preparing);

    let window = compact_window(total)?;

    let early_messages = &messages[window.head_protect..window.split_point];

    // 尝试 LLM 摘要，失败则回退到本地
    let early_tokens = crate::domain::token_budget::estimate_messages_tokens(early_messages);
    let summary = match client {
        Some(client) => {
            if early_tokens > COMPACT_CHUNK_TARGET_TOKENS {
                match compact_messages_map_reduce(client, early_messages, progress, context_size, cancel).await {
                    Ok(text) => text,
                    Err(_) => build_summary_text(early_messages),
                }
            } else {
                emit_progress(progress, CompactStage::Summarizing);
                match llm_compact(client, early_messages, context_size, cancel).await {
                    Ok(text) => text,
                    Err(_) => build_summary_text(early_messages),
                }
            }
        }
        None => build_summary_text(early_messages),
    };

    emit_progress(progress, CompactStage::Finalizing);

    // recent tail：split_point 到末尾的原始消息
    let mut recent = messages[window.split_point..].to_vec();
    sanitize_tool_pairs(&mut recent);
    // 截断 recent tail 中超阈值的 ToolResult，避免大输出导致 compact 后仍超 context 阈值。
    placeholder_tool_results(&mut recent);

    Some(CompactResult {
        summary,
        recent_messages: recent,
    })
}

/// 底层 LLM 调用：发送 request 消息列表，流式收集文本并解析 `<summary>` 标签。
async fn llm_generate(
    client: &provider::LlmClient,
    request: Vec<Message>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let collected = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let collected_clone = collected.clone();
    {
        let mut handler = provider::CallbackHandler::new(Box::new(move |text: &str| {
            if let Ok(mut guard) = collected_clone.lock() {
                guard.push_str(text);
            }
        }));

        client
            .stream_message(
                client.default_scope(),
                &[],
                &request,
                &[],
                &mut handler,
                cancel,
            )
            .await
            .map_err(|e| format!("LLM call failed: {e}"))?;
    }

    let full_text = collected.lock().map_err(|e| format!("Lock error: {e}"))?;
    let summary = parse_compact_response(full_text.as_str());
    if summary.is_empty() {
        return Err("LLM returned empty summary".into());
    }
    Ok(summary)
}

/// 调用 LLM 对 early_messages 生成单次压缩摘要。
async fn llm_compact(
    client: &provider::LlmClient,
    early_messages: &[Message],
    context_size: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let mut request = build_compact_request(early_messages, context_size);
    request.push(Message::user(format!(
        "{}\n\n这里是要总结的对话：",
        COMPACT_PROMPT
    )));
    llm_generate(client, request, cancel).await
}

/// 将消息列表按 token 预算分块（不拆分单条消息）。
fn split_messages_into_chunks(messages: &[Message], target_tokens: usize) -> Vec<Vec<Message>> {
    use crate::domain::token_budget::estimate_message_tokens;

    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_tokens = 0usize;

    for msg in messages {
        let msg_tokens = estimate_message_tokens(msg);
        if current_tokens + msg_tokens > target_tokens && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current.push(msg.clone());
        current_tokens += msg_tokens;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// map-reduce 式压缩：分块独立摘要 → 合并为最终摘要。
///
/// 当 early_messages 很大时，单次 LLM compact 会因输入过长而摘要质量下降。
/// 改为分块（map）再合并（reduce）：
/// 1. map: 按 token 预算分 N 块，每块独立调用 `llm_compact`。
/// 2. reduce: 把 N 个子摘要合并，再次调用 LLM 生成连贯的最终摘要。
async fn compact_messages_map_reduce(
    client: &provider::LlmClient,
    early_messages: &[Message],
    progress: Option<&dyn CompactProgressFn>,
    context_size: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    use crate::domain::token_budget::estimate_messages_tokens;

    let chunks = split_messages_into_chunks(early_messages, COMPACT_CHUNK_TARGET_TOKENS);
    let total_chunks = chunks.len();
    log::info!(
        target: "aemeath:agent:runtime",
        "map-reduce compact: {} chunks from {} messages ({} tokens)",
        total_chunks,
        early_messages.len(),
        estimate_messages_tokens(early_messages),
    );

    // map: 每个 chunk 独立摘要
    let mut sub_summaries = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        emit_progress_chunk(progress, CompactStage::Summarizing, i + 1, total_chunks);
        let summary = llm_compact(client, chunk, context_size, cancel).await?;
        sub_summaries.push(summary);
        log::info!(
            target: "aemeath:agent:runtime",
            "map-reduce compact: chunk {}/{} done",
            i + 1,
            total_chunks,
        );
    }

    // 只有 1 块时无需 reduce
    if sub_summaries.len() <= 1 {
        return Ok(sub_summaries.into_iter().next().unwrap_or_default());
    }

    // reduce: 合并子摘要
    let combined = sub_summaries
        .iter()
        .enumerate()
        .map(|(i, s)| format!("## Part {} summary\n\n{s}", i + 1))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    let prompt = format!(
        "{COMPACT_PROMPT}\n\n以下是对话的多个分段摘要，请合并为一份连贯的最终摘要：\n\n<sub-summaries>\n{combined}\n</sub-summaries>\n\nWrite your summary inside <summary> tags."
    );

    llm_generate(client, vec![Message::user(prompt)], cancel).await
}

#[cfg(test)]
#[path = "compact_summary_tests.rs"]
mod compact_summary_tests;
