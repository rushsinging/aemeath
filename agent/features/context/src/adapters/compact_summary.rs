//! 消息压缩 — 本地文本摘要和 LLM 摘要
//!
//! 提供 `compact_messages` 作为本地压缩入口，以及 LLM 压缩相关的
//! 请求构建 / 响应解析 / 摘要文本生成。

use crate::domain::compact::{sanitize_tool_pairs, CompactProgressFn, CompactStage};
use async_trait::async_trait;
use futures_util::StreamExt;
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
                        target: crate::LOG_TARGET,
                        "compact placeholder ToolResult {tool_use_id}",
                    );
                }
            }
        }
    }
}

/// Compact 进度回调 trait 的域定义见 `crate::domain::compact::CompactProgressFn`
/// （#1500 上移，adapter 层不再重复定义）。
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

/// Context-owned external-detail port for generating LLM text during compaction.
///
/// The implementor (typically a runtime adapter wrapping a `ProviderPort`)
/// invokes the LLM with the supplied request messages and returns the full
/// text output. Context retains ownership of prompt construction, map-reduce
/// orchestration, summary parsing, cancellation propagation, and fallback.
///
/// This trait is the sole boundary through which compact logic reaches an LLM;
/// production code must not depend on a concrete Provider construction handle.
#[async_trait]
pub trait CompactGenerator: Send + Sync {
    /// Generate the full text output for the given request messages.
    ///
    /// Returns the concatenated text content on success, or an error message
    /// describing the failure. Cancellation is cooperative via `cancel`.
    async fn generate(
        &self,
        request: Vec<Message>,
        cancel: &CancellationToken,
    ) -> Result<String, String>;
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

Budget: Aim for up to {BUDGET} tokens. This checkpoint replaces the original messages, so preserve continuation-critical semantics while avoiding duplicate detail.

<instructions>
Produce a checkpoint using the EXACT structure below inside `<summary>` tags.

## Immutable Constraints
Long-lived user constraints, permission boundaries, and prohibited actions. Later corrections supersede earlier conflicts.

## Current Objective
Exactly one current objective at the user's requested action level.

## Committed Facts
Only facts supported by tool, commit, test, or durable persistence evidence.

## Uncommitted Working Set
Current branch changes, failing tests, active tasks, and immediate local context.

## Open Decisions / Risks
Unresolved questions, blockers, uncertainty, and unverified reports.

## Resume Cursor
Worktree, branch, current task, target files, exactly one Next action, and explicit prohibited actions.

## Required Revalidation
GitHub, CI, remote branch, worktree current state, and every other dynamic fact that must be queried again before mutation.

## Archived Milestones
One-line results with stable commit, PR, or issue references; never copy process transcripts.

## Continuation Status
Exactly one of: Continue, Waiting for User, or Completed. Add one short reason after the status.

Rules:
- Be specific: include file paths, function names, variable names.
- Preserve the requested action level exactly. NEVER upgrade inspect, diagnose, explain, review, or design into implement, edit, commit, push, merge, or close.
- Consolidate user inputs chronologically; later corrections supersede earlier conflicting instructions.
- State each detailed fact in one authoritative section; do not duplicate it elsewhere.
- Put GitHub, CI, remote branch, worktree, and other current external state under Required Revalidation.
- Resume Cursor MUST contain exactly one Next action and explicit prohibited actions.
- Archived Milestones contain one-line stable references, not process transcripts.
- Distinguish facts from inference. Do not claim work was completed unless the history shows it.
- If Continuation Status is Continue, the agent must execute the Resume Cursor Next action after required revalidation without waiting for a new user instruction.
- Use Waiting for User only when an explicit approval, choice, missing input, or new authority is genuinely required.
- Use Completed only when the user's requested outcome has already been delivered and no work remains.
- Do NOT include raw tool output or tool call details — focus on semantic meaning.
- Do NOT ask clarifying questions or say "no task found" — this is history compression, not a chat.
- Each section can be empty if not applicable, but include the heading.
</instructions>

Here is the PAST conversation history to compress:
"#;

/// previous_summary 允许嵌入的最大字符数（domain 单一真相，见 token_budget）。
pub const FALLBACK_PREVIOUS_SUMMARY_CAP: usize =
    crate::domain::token_budget::FALLBACK_PREVIOUS_SUMMARY_CAP;

/// 汇总后的最终摘要超过预算时，最多再压的迭代次数（#1486 收敛迭代）。
const MAX_REDUCE_REFRESH_ROUNDS: usize = 3;

/// 再压提示词的预算缩减系数（#1490）：给 LLM 的提示预算 =
/// `summary_budget × REFRESH_BUDGET_RATIO`，为 LLM 实际输出超出提示预算
/// 留余量，保证真实输出落在 summary_budget 内。
const REFRESH_BUDGET_RATIO: usize = 8; // × 0.8

/// 再压（refresh）专用提示词（#1490）。
///
/// 与通用 [`COMPACT_PROMPT`] 相反：再压必须**激进压缩**，丢弃细节，
/// 只保留决策/状态实质；预算为硬约束（MUST NOT exceed），且按
/// `summary_budget × 0.8` 提示，为 LLM 实际输出超出提示预算留余量，
/// 保证真实输出落在 summary_budget 内。
const COMPACT_REFRESH_PROMPT: &str = r#"You are compressing an existing conversation summary. The summary below is TOO LONG and must be reduced.

CRITICAL BUDGET: The compressed output MUST NOT exceed {BUDGET} tokens. If you cannot fit everything, drop details — the summary is context, not a transcript.

<instructions>
Produce a compressed summary using the EXACT structure below inside <summary> tags.

## Immutable Constraints
## Current Objective
## Committed Facts
## Uncommitted Working Set
## Open Decisions / Risks
## Resume Cursor
## Required Revalidation
## Archived Milestones
## Continuation Status

Rules:
- Compress by semantic priority, not by truncating the head or tail of the whole checkpoint.
- Preserve the requested action level exactly. NEVER upgrade inspect, diagnose, explain, review, or design into implement, edit, commit, push, merge, or close.
- State each detailed fact once in its authoritative section.
- Put dynamic GitHub, CI, remote branch, and worktree state under Required Revalidation.
- Resume Cursor MUST contain exactly one Next action and explicit prohibited actions.
- Archived Milestones contain one-line stable references, not process transcripts.
- Each section can be empty if not applicable, but include the heading.
- The output MUST be shorter than the input and MUST NOT exceed {BUDGET} tokens.
</instructions>

Here is the summary to compress:
"#;

/// 构建再压提示词（#1490）：硬预算 + 激进压缩指令。
///
/// `budget` 为真实 summary 预算；提示词内按 `× REFRESH_BUDGET_RATIO` 缩减，
/// 为 LLM 实际输出超出提示预算留余量，保证真实输出落在 `budget` 内。
pub(crate) fn build_refresh_prompt(summary: &str, budget: usize) -> String {
    let prompt_budget = budget * REFRESH_BUDGET_RATIO / 10;
    format!(
        "{COMPACT_REFRESH_PROMPT}\n<current_summary>\n{summary}\n</current_summary>\n\nWrite your summary inside <summary> tags."
    )
    .replace("{BUDGET}", &prompt_budget.to_string())
}

pub(crate) fn normalize_generated_checkpoint(
    summary: &str,
    budget: usize,
) -> Result<String, String> {
    let (checkpoint_text, task_state) =
        crate::domain::compact::split_checkpoint_and_task_state(summary);
    let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(checkpoint_text)
        .map_err(|error| error.to_string())?;
    let mut normalized = checkpoint
        .normalize_to_budget(budget)
        .map_err(|error| error.to_string())?
        .render();
    if let Some(task_state) = task_state.filter(|state| !state.is_empty()) {
        normalized.push_str("\n\n## Current Task State\n");
        normalized.push_str(task_state);
    }
    Ok(normalized)
}

/// 调用 LLM 对当前 summary 再压一次（#1490）。
async fn llm_refresh(
    generator: &dyn CompactGenerator,
    summary: &str,
    budget: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let prompt = build_refresh_prompt(summary, budget);
    llm_generate(generator, vec![Message::user(prompt)], cancel).await
}

/// 使用本地文本提取压缩消息（LLM 不可用时的回退方案）。
///
/// 调用方已经根据归一化的 Provider usage 或无 usage 时的完整估算完成决策；
/// 本函数只执行压缩。返回 `None` 仅表示消息太少，无法形成有效压缩窗口。
/// summary 不再注入 messages，走 system 通道。
pub fn compact_messages(messages: &[Message]) -> Option<CompactResult> {
    let total = messages.len();
    let window = compact_window(total)?;
    if total <= 4 {
        return None;
    }

    // summary 必须覆盖所有将从 active messages 移除的内容。
    // `head_protect` 只参与窗口边界计算；头部消息不进入 recent tail，
    // 因此也必须进入 summary，避免首条用户请求永久丢失。
    let early_messages = &messages[..window.split_point]; // allow unsafe_text_op: Vec slice
    let summary = build_summary_text(early_messages, None);

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
        .map(|window| messages[..window.split_point].to_vec()) // allow unsafe_text_op: Vec slice
        .unwrap_or_default()
}

/// 从早期对话历史构建 LLM 压缩请求消息。
pub fn build_compact_request(
    early_messages: &[Message],
    previous_summary: Option<&str>,
    context_size: usize,
) -> Vec<Message> {
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

    let previous_summary = previous_summary
        .filter(|summary| !summary.trim().is_empty())
        .map(|summary| {
            let (checkpoint_text, _) =
                crate::domain::compact::split_checkpoint_and_task_state(summary);
            let checkpoint = crate::domain::compact::ContinuationCheckpoint::parse(checkpoint_text)
                .unwrap_or_else(|_| {
                    crate::domain::compact::ContinuationCheckpoint::from_legacy_summary(
                        checkpoint_text,
                    )
                });
            let previous_budget = crate::domain::token_budget::summary_budget(context_size)
                .min(FALLBACK_PREVIOUS_SUMMARY_CAP / 4);
            let checkpoint = checkpoint
                .normalize_to_budget(previous_budget)
                .unwrap_or_else(|error| {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[compact] previous checkpoint 无法收敛到预算：{error}",
                    );
                    crate::domain::compact::ContinuationCheckpoint::from_legacy_summary(
                        "## User Requests\n- Revalidate the previous compact checkpoint before continuing.\n\n## Next Action\n- Revalidate prior constraints and current objective.\n\n## Continuation Status\nWaiting for User — protected checkpoint content exceeded its budget.",
                    )
                })
                .render();
            format!(
                "<previous_checkpoint>\n{checkpoint}\n</previous_checkpoint>\n\n\
                 The previous checkpoint is authoritative for durable constraints and history. \
                 Merge it with newer history, but keep dynamic state under Required Revalidation \
                 and replace its Resume Cursor when newer user input supersedes it.\n\n"
            )
        })
        .unwrap_or_default();
    let prompt = format!(
        "{COMPACT_PROMPT}\n{previous_summary}<conversation_history>\n{conversation_text}</conversation_history>\n\nCompress this history into a summary now. Write your summary inside <summary> tags.",
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
                return response_text[start..end].trim().to_string(); // allow unsafe_text_op: find offset (char boundary)
            }
        }
    }
    // 回退：使用整个响应
    response_text.trim().to_string()
}

/// 从早期消息构建本地文本摘要（回退方案，无 LLM 调用）。
pub fn build_summary_text(messages: &[Message], previous_summary: Option<&str>) -> String {
    let mut user_requests = Vec::new();
    let mut assistant_reports = Vec::new();
    let mut observed_tool_invocations = Vec::new();
    let mut last_text: Option<(Role, String)> = None;

    for msg in messages {
        let text = msg.text_content();
        if !text.is_empty() {
            let truncated = if text.len() > 200 {
                format!("{}...", slice_head(&text, 200))
            } else {
                text
            };
            last_text = Some((msg.role.clone(), truncated.clone()));
            match msg.role {
                Role::User => user_requests.push(format!("- {truncated}")),
                Role::Assistant => {
                    let label = if indicates_completion(&truncated) {
                        "Assistant-reported completion (unverified)"
                    } else {
                        "Unverified assistant report"
                    };
                    assistant_reports.push(format!("- {label}: {truncated}"));
                }
            }
        }

        // 工具调用只证明发起过调用，不能证明结果成功或工作已完成。
        let tool_uses = msg.extract_tool_uses();
        if !tool_uses.is_empty() {
            let tool_names: Vec<&str> = tool_uses.iter().map(|(_, name, _)| *name).collect();
            observed_tool_invocations.push(format!(
                "- Observed tool invocation (outcome not established): {}",
                tool_names.join(", ")
            ));
        }
    }

    let user_requests = if user_requests.is_empty() {
        "- Unknown: no user text was available in the fallback input.".to_string()
    } else {
        user_requests.join("\n")
    };
    let mut reported_context = assistant_reports;
    reported_context.extend(observed_tool_invocations);
    let work_completed = if reported_context.is_empty() {
        "- No completed work could be established from the fallback input.".to_string()
    } else {
        reported_context.join("\n")
    };
    let previous_checkpoint = previous_summary
        .filter(|summary| !summary.trim().is_empty())
        .map(|summary| {
            let (checkpoint_text, _) =
                crate::domain::compact::split_checkpoint_and_task_state(summary);
            crate::domain::compact::ContinuationCheckpoint::parse(checkpoint_text)
                .unwrap_or_else(|_| {
                    crate::domain::compact::ContinuationCheckpoint::from_legacy_summary(
                        checkpoint_text,
                    )
                })
                .normalize_to_budget(FALLBACK_PREVIOUS_SUMMARY_CAP / 4)
                .unwrap_or_else(|_| {
                    crate::domain::compact::ContinuationCheckpoint::from_legacy_summary(
                        "## User Requests\n- Revalidate the previous compact checkpoint.\n\n## Next Action\n- Revalidate prior constraints and current objective.\n\n## Continuation Status\nWaiting for User — protected checkpoint content exceeded its budget.",
                    )
                })
                .render()
        });
    let (next_action, continuation_status) = fallback_continuation(last_text.as_ref());
    let current_objective = user_requests
        .lines()
        .last()
        .unwrap_or("- Unknown current objective.")
        .to_string();
    let current_checkpoint = crate::domain::compact::ContinuationCheckpoint::from_sections(
        crate::domain::compact::CheckpointSections {
            immutable_constraints: vec![
                "- Preserve the user's requested action level; do not infer new authority."
                    .to_string(),
            ],
            current_objective: vec![current_objective],
            committed_facts: vec![
                "- No completed work could be established from the fallback input.".to_string(),
            ],
            uncommitted_working_set: user_requests.lines().map(str::to_string).collect(),
            open_decisions_and_risks: std::iter::once(
                "- Local text-compaction path used; all reports below are unverified."
                    .to_string(),
            )
            .chain(work_completed.lines().map(str::to_string))
            .collect(),
            resume_cursor_lines: vec![
                "- Prohibited: do not claim completion, commit, push, merge, or close without evidence and authority."
                    .to_string(),
            ],
            next_action,
            required_revalidation: vec![
                "- Revalidate Git, GitHub, CI, worktree, test, and task current state before mutation."
                    .to_string(),
            ],
            archived_milestones: vec![
                "- No stable milestone references established by fallback.".to_string(),
            ],
            status: continuation_status.0,
            status_reason: Some(continuation_status.1),
        },
    )
    .expect("fallback checkpoint typed fields must be valid");
    let checkpoint = previous_checkpoint.map_or(current_checkpoint.clone(), |previous| {
        crate::domain::compact::ContinuationCheckpoint::parse(&previous)
            .expect("normalized previous checkpoint must parse")
            .merge_fallback_update(current_checkpoint)
    });
    checkpoint
        .normalize_to_budget(FALLBACK_PREVIOUS_SUMMARY_CAP / 4)
        .unwrap_or_else(|_| {
            crate::domain::compact::ContinuationCheckpoint::from_legacy_summary(
                "## User Requests\n- Revalidate the compact fallback.\n\n## Next Action\n- Revalidate the compact fallback.\n\n## Continuation Status\nWaiting for User — fallback checkpoint exceeded its budget.",
            )
        })
        .render()
}

fn indicates_waiting_for_user(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "等待你",
        "等你确认",
        "需要你确认",
        "等待用户",
        "waiting for user",
        "waiting for your",
        "awaiting approval",
        "need your approval",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn indicates_completion(text: &str) -> bool {
    let lower = text.to_lowercase();
    if [
        "not completed",
        "not finished",
        "not merged",
        "未完成",
        "尚未完成",
        "没有完成",
        "未合入",
        "没有合入",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return false;
    }
    [
        "已完成",
        "已经完成",
        "完成并通过",
        "已合入",
        "completed",
        "finished",
        "merged",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn fallback_continuation(
    last_text: Option<&(Role, String)>,
) -> (String, (crate::domain::compact::ContinuationStatus, String)) {
    match last_text {
        Some((Role::User, text)) => (
            format!("Address the latest user request without expanding its scope: {text}"),
            (
                crate::domain::compact::ContinuationStatus::Continue,
                "the latest compacted message is an unresolved user request.".to_string(),
            ),
        ),
        Some((Role::Assistant, text)) if indicates_waiting_for_user(text) => (
            format!("Wait for the user input or approval reported here: {text}"),
            (
                crate::domain::compact::ContinuationStatus::WaitingForUser,
                "the assistant explicitly reported that user input or approval is required."
                    .to_string(),
            ),
        ),
        Some((Role::Assistant, text)) if indicates_completion(text) => (
            format!("Wait for the user to confirm completion or request follow-up; reported state: {text}"),
            (
                crate::domain::compact::ContinuationStatus::WaitingForUser,
                "the assistant reported completion, but deterministic fallback cannot verify delivery or whether follow-up remains."
                    .to_string(),
            ),
        ),
        Some((Role::Assistant, text)) => (
            format!("Wait for a new user instruction; last assistant report: {text}"),
            (
                crate::domain::compact::ContinuationStatus::WaitingForUser,
                "no unambiguous pending user action can be established from the fallback input."
                    .to_string(),
            ),
        ),
        None => (
            "Wait for a new user instruction because no actionable text was available.".to_string(),
            (
                crate::domain::compact::ContinuationStatus::WaitingForUser,
                "deterministic fallback found no actionable user or assistant text.".to_string(),
            ),
        ),
    }
}

/// 使用 LLM 进行语义化压缩（对早期消息生成结构化摘要）。
///
/// 如果 LLM 调用失败，回退到本地 `build_summary_text`。
/// 返回 `Some(CompactResult)` 表示发生了压缩；`None` 表示无需压缩。
/// summary 走 system 通道，不注入 messages。
pub async fn compact_messages_with_llm(
    messages: &[Message],
    previous_summary: Option<&str>,
    context_size: usize,
    generator: Option<&dyn CompactGenerator>,
    progress: Option<&dyn CompactProgressFn>,
    cancel: &CancellationToken,
) -> Option<CompactResult> {
    // should_compact 判定已在调用方（状态机 needs_compaction）完成。
    // 进入此函数即直接执行 compact 管线，不再二次检查。
    let total = messages.len();
    if total <= 4 {
        return None;
    }

    emit_progress(progress, CompactStage::Preparing);

    let window = compact_window(total)?;

    // 与 recent tail 互补：所有不再保留的消息都必须参与 summary。
    let early_messages = &messages[..window.split_point]; // allow unsafe_text_op: Vec slice

    // 尝试 LLM 摘要，失败则回退到本地
    let early_tokens = crate::domain::token_budget::estimate_messages_tokens(early_messages);
    let previous_len = previous_summary.map(str::len).unwrap_or(0);
    log::info!(
        target: crate::LOG_TARGET,
        "[compact] 开始：messages={total} early={} early_tokens={early_tokens} previous_summary={previous_len} chars mode={}",
        early_messages.len(),
        if generator.is_some() { "llm" } else { "local" },
    );
    let summary = match generator {
        Some(generator) => {
            let result = if early_tokens > chunk_target_tokens(context_size) {
                compact_messages_map_reduce(
                    generator,
                    early_messages,
                    previous_summary,
                    progress,
                    context_size,
                    cancel,
                )
                .await
            } else {
                emit_progress(progress, CompactStage::Summarizing);
                llm_compact(
                    generator,
                    early_messages,
                    previous_summary,
                    context_size,
                    cancel,
                )
                .await
            };
            match result {
                Ok(text) => match normalize_generated_checkpoint(
                    &text,
                    crate::domain::token_budget::summary_budget(context_size),
                ) {
                    Ok(checkpoint) => checkpoint,
                    Err(error) => {
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "[compact] LLM checkpoint 不合规，回退本地路径：{error}",
                        );
                        build_summary_text(early_messages, previous_summary)
                    }
                },
                Err(error) => {
                    // #1486 可跟踪性：LLM 摘要失败原因必须记录，否则无法判断
                    // 为何回退本地路径（超限 / 空摘要 / provider 错误）。
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[compact] LLM 摘要失败，回退本地路径：{error}",
                    );
                    build_summary_text(early_messages, previous_summary)
                }
            }
        }
        None => build_summary_text(early_messages, previous_summary),
    };

    emit_progress(progress, CompactStage::Finalizing);

    // recent tail：split_point 到末尾的原始消息
    let mut recent = messages[window.split_point..].to_vec();
    sanitize_tool_pairs(&mut recent);
    // 截断 recent tail 中超阈值的 ToolResult，避免大输出导致 compact 后仍超 context 阈值。
    placeholder_tool_results(&mut recent);

    log::info!(
        target: crate::LOG_TARGET,
        "[compact] 完成：summary={} chars recent_messages={}",
        summary.len(),
        recent.len(),
    );

    Some(CompactResult {
        summary,
        recent_messages: recent,
    })
}

/// 底层 LLM 调用：通过 `CompactGenerator` 发送 request 消息列表，收集文本并解析 `<summary>` 标签。
async fn llm_generate(
    generator: &dyn CompactGenerator,
    request: Vec<Message>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let full_text = generator.generate(request, cancel).await?;
    let summary = parse_compact_response(&full_text);
    if summary.is_empty() {
        return Err("LLM returned empty summary".into());
    }
    Ok(summary)
}

/// 调用 LLM 对 early_messages 生成单次压缩摘要。
async fn llm_compact(
    generator: &dyn CompactGenerator,
    early_messages: &[Message],
    previous_summary: Option<&str>,
    context_size: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let request = build_compact_request(early_messages, previous_summary, context_size);
    llm_generate(generator, request, cancel).await
}

/// 单块摘要目标 token 数：按上下文总长度比例切（#1486，见 token_budget）。
fn chunk_target_tokens(context_size: usize) -> usize {
    crate::domain::token_budget::compact_chunk_target_tokens(context_size)
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
/// 1. map: 按 token 预算分 N 块，每块独立调用 `llm_compact`，**并发 3-5**（视块数而定）。
/// 2. reduce: 把 N 个子摘要合并，再次调用 LLM 生成连贯的最终摘要。
/// 3. 收敛：最终摘要超过预算时再压一次（最多 `MAX_REDUCE_REFRESH_ROUNDS` 轮），
///    避免超大摘要直接注入 system（#1486）。
async fn compact_messages_map_reduce(
    generator: &dyn CompactGenerator,
    early_messages: &[Message],
    previous_summary: Option<&str>,
    progress: Option<&dyn CompactProgressFn>,
    context_size: usize,
    cancel: &CancellationToken,
) -> Result<String, String> {
    use crate::domain::token_budget::estimate_messages_tokens;

    let chunk_target = chunk_target_tokens(context_size);
    let chunks = split_messages_into_chunks(early_messages, chunk_target);
    let total_chunks = chunks.len();
    // map: 每个 chunk 独立摘要，按块数决定并发上限（3-5）。
    // 块数越多并发越高，但不超过 5；避免同时打爆 provider。
    let concurrency = match total_chunks {
        0 => 1,
        1..=3 => 3,
        4..=6 => 4,
        _ => 5,
    };
    log::info!(
        target: crate::LOG_TARGET,
        "[compact] map-reduce：{total_chunks} chunks，{} messages，{} tokens，单块目标 {chunk_target}，并发上限 {concurrency}",
        early_messages.len(),
        estimate_messages_tokens(early_messages),
    );

    let mut sub_summaries = Vec::with_capacity(total_chunks);
    let futures = chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            emit_progress_chunk(progress, CompactStage::Summarizing, i + 1, total_chunks);
            let previous_for_chunk = (i == 0).then_some(previous_summary).flatten();
            llm_compact(generator, chunk, previous_for_chunk, context_size, cancel)
        })
        .collect::<Vec<_>>();
    let mut in_flight = futures_util::stream::iter(futures).buffer_unordered(concurrency);
    let mut index = 0usize;
    while let Some(summary) = in_flight.next().await {
        sub_summaries.push(summary?);
        index += 1;
        log::debug!(
            target: crate::LOG_TARGET,
            "[compact] chunk {index}/{total_chunks} 摘要完成",
        );
    }

    // 只有 1 块时无需 reduce
    if sub_summaries.len() <= 1 {
        return Ok(sub_summaries.into_iter().next().unwrap_or_default());
    }

    // reduce: 合并子摘要，调用 LLM 生成连贯最终摘要
    let combined = sub_summaries
        .iter()
        .enumerate()
        .map(|(i, s)| format!("## Part {} summary\n\n{s}", i + 1))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    let prompt = format!(
        "{COMPACT_PROMPT}\n\n以下是对话的多个分段摘要，请合并为一份连贯的最终摘要：\n\n<sub-summaries>\n{combined}\n</sub-summaries>\n\nWrite your summary inside <summary> tags."
    );
    let mut final_summary = llm_generate(generator, vec![Message::user(prompt)], cancel).await?;
    log::info!(
        target: crate::LOG_TARGET,
        "[compact] reduce 合并完成：{} chars（预算 {} tokens）",
        final_summary.len(),
        crate::domain::token_budget::summary_budget(context_size),
    );

    // 收敛迭代：合并结果超预算时再压一次，直到有界或达到轮数上限（#1486/#1490）。
    // 再压使用专用提示词（硬预算 + 激进压缩），提示预算按 summary_budget×0.8
    // 留余量；收敛判定统一用 estimate_tokens，连续两轮未缩小才停（容忍 LLM
    // 输出噪音一轮），且未缩小轮次不采用更差的输出。
    let budget = crate::domain::token_budget::summary_budget(context_size);
    let mut rounds_without_shrink = 0usize;
    for round in 1..=MAX_REDUCE_REFRESH_ROUNDS {
        if crate::domain::token_budget::estimate_tokens(&final_summary) <= budget {
            break;
        }
        let tokens_before = crate::domain::token_budget::estimate_tokens(&final_summary);
        let refreshed = llm_refresh(generator, &final_summary, budget, cancel).await?;
        let tokens_after = crate::domain::token_budget::estimate_tokens(&refreshed);
        log::info!(
            target: crate::LOG_TARGET,
            "[compact] 汇总超预算，再压 round {round}：{tokens_before} -> {tokens_after} tokens（预算 {budget}）",
        );
        if tokens_after >= tokens_before {
            rounds_without_shrink += 1;
            if rounds_without_shrink >= 2 {
                // 连续两轮未缩小：停止迭代，保持当前（更优）summary，避免采用更差输出。
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[compact] 再压连续两轮未缩小（{tokens_after} tokens），停止迭代，保留 {tokens_before} tokens 的当前 summary",
                );
                break;
            }
            // 第一轮噪音：不采用 refreshed，基于原 summary 再试一轮。
            log::debug!(
                target: crate::LOG_TARGET,
                "[compact] 再压本轮未缩小（{tokens_after} >= {tokens_before}），下一轮重试",
            );
            continue;
        }
        rounds_without_shrink = 0;
        final_summary = refreshed;
    }
    Ok(final_summary)
}

#[cfg(test)]
#[path = "compact_summary_tests.rs"]
mod compact_summary_tests;
