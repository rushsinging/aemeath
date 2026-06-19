use crate::business::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::{
    ChatEventSink, ChatLoopFsm, ChatLoopState, ChatLoopTransition, RuntimeStreamEvent,
    RuntimeTurnContext,
};
use crate::LOG_TARGET;
use hook::api::{is_blocking, HookData, HookJsonOutput, HookResult, HookRunner, StopHookData};
use share::config::hooks::HookEvent;
use std::path::PathBuf;
use storage::api::{BatchStatus, TaskStore};

const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;

/// Stop hook 连续阻断的最大次数，超过后强制停止以避免无限循环（#372）。
pub(crate) const MAX_STOP_HOOK_BLOCKS: usize = 5;

/// Run stop/failure hooks and return feedback if the loop should continue.
///
/// **Bug #49 note**: input queue draining happens *before* this function is
/// called (in `stream.rs`). If queued input exists, the loop `continue`s
/// without reaching here. When a stop hook blocks the stop, the returned
/// feedback is injected as a system-reminder and the loop `continue`s — the
/// next iteration will again drain the queue first.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_main_loop<S>(
    outcome: &AgentRunOutcome,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &HookRunner,
    session_id: &str,
    context: &RuntimeTurnContext,
    _task_store: &TaskStore,
    language: &str,
) -> Option<String>
where
    S: ChatEventSink,
{
    log_agent_outcome(outcome, session_id);

    match &outcome.status {
        AgentRunStatus::Completed | AgentRunStatus::MaxTurns => {
            run_stop_hook_before_finish(outcome, sink, hook_ui, hook_runner, session_id, language)
                .await
        }
        AgentRunStatus::Cancelled => {
            let _ = sink
                .send_event(RuntimeStreamEvent::Done {
                    context: context.clone(),
                })
                .await;
            None
        }
        AgentRunStatus::ApiError(_) | AgentRunStatus::TimedOut => {
            let failure_results = hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::StopFailure,
                    None,
                    HookData::Stop(StopHookData {
                        turns: outcome.turns,
                    }),
                )
                .await;
            // #372: StopFailure hook 阻断时回流反馈，让 loop 有机会恢复
            if let Some(feedback) = stop_hook_feedback(&failure_results, session_id, language).await
            {
                return Some(feedback);
            }
            let _ = sink
                .send_event(RuntimeStreamEvent::Done {
                    context: context.clone(),
                })
                .await;
            None
        }
    }
}

pub(crate) async fn run_stop_hook_before_finish<S>(
    outcome: &AgentRunOutcome,
    _sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &HookRunner,
    session_id: &str,
    language: &str,
) -> Option<String>
where
    S: ChatEventSink,
{
    let stop_results = hook_ui
        .run_json(
            hook_runner,
            HookEvent::Stop,
            None,
            HookData::Stop(StopHookData {
                turns: outcome.turns,
            }),
        )
        .await;
    if let Some(feedback) = stop_hook_feedback(&stop_results, session_id, language).await {
        // hook_ui.run_json 已在内部发送 HookEvent（含 Blocked 状态），
        // 此处不再重复发送，避免 TUI 显示两次 "Hook blocked: Stop"。
        return Some(feedback);
    }
    None
}

pub(crate) async fn finish_completed_loop<S>(
    outcome: &AgentRunOutcome,
    sink: &S,
    context: &RuntimeTurnContext,
    task_store: &TaskStore,
) where
    S: ChatEventSink,
{
    let _ = sink
        .send_event(RuntimeStreamEvent::DoneWithDuration {
            context: context.clone(),
            duration: outcome.duration,
        })
        .await;

    if let Some(active) = task_store.active_list().await {
        if task_store.is_batch_completed(active.id).await {
            task_store
                .set_batch_status(active.id, BatchStatus::Archived)
                .await;
            log::info!(target: LOG_TARGET,
                "[task_list_archived] batch_id={}, status=archived, reason=all_tasks_completed",
                active.id
            );
        }
    }
}

async fn stop_hook_feedback(
    hook_results: &[(
        share::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
    session_id: &str,
    language: &str,
) -> Option<String> {
    let (entry, result, json) = stop_hook_blocking_result(hook_results)?;
    let details = hook_feedback_details(result, json, session_id, &entry.command, language).await;
    let template = match language {
        "zh" => "Stop hook 阻止了停止。你现在还不能结束本轮处理。\n你 MUST 先满足下面 Stop hook 的要求，然后才能再次尝试停止。\n命令：{cmd}\n{details}",
        _ => "Stop hook prevented stopping. You cannot finish this turn yet.\nYou MUST first satisfy the Stop hook requirement below, then attempt to stop again.\nCommand: {cmd}\n{details}",
    };
    Some(
        template
            .replace("{cmd}", &entry.command)
            .replace("{details}", &details),
    )
}

fn stop_hook_blocking_result(
    hook_results: &[(
        share::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
) -> Option<(
    &share::config::hooks::HookEntry,
    &HookResult,
    &Option<HookJsonOutput>,
)> {
    hook_results
        .iter()
        .filter(|(_, result, json)| is_blocking(result, json))
        .find(|(_, result, json)| has_hook_feedback(result, json))
        .or_else(|| {
            hook_results
                .iter()
                .find(|(_, result, json)| is_blocking(result, json))
        })
        .map(|(entry, result, json)| (entry, result, json))
}

fn has_hook_feedback(result: &HookResult, json: &Option<HookJsonOutput>) -> bool {
    hook_json_reason(json).is_some()
        || non_empty_text(result.error.as_deref().unwrap_or_default()).is_some()
        || non_empty_text(&result.output).is_some()
}

fn hook_json_reason(json: &Option<HookJsonOutput>) -> Option<String> {
    json.as_ref().and_then(|j| {
        j.reason
            .clone()
            .or_else(|| j.system_message.clone())
            .or_else(|| j.additional_context.clone())
            .or_else(|| j.stop_reason.clone())
    })
}

#[allow(clippy::too_many_arguments)]
async fn hook_feedback_details(
    result: &HookResult,
    json: &Option<HookJsonOutput>,
    session_id: &str,
    command: &str,
    language: &str,
) -> String {
    let (labels, lang) = match language {
        "zh" => (
            HookFeedbackLabels {
                json_feedback: "JSON 反馈：\n{}",
                stderr_error: "stderr/错误：\n{}",
                stdout: "stdout：\n{}",
                no_reason: "Stop hook 阻止了停止，但没有提供原因",
                output_too_long_file: "hook 输出过长，已保存到文件：{}\n请读取该文件查看完整 stdout/stderr。",
                output_too_long_preview: "hook 输出过长，以下为前 {n} 字节预览：\n{preview}",
            },
            "zh",
        ),
        _ => (
            HookFeedbackLabels {
                json_feedback: "JSON feedback:\n{}",
                stderr_error: "stderr/error:\n{}",
                stdout: "stdout:\n{}",
                no_reason: "Stop hook prevented stopping but provided no reason",
                output_too_long_file: "Hook output too long, saved to file: {}\nPlease read that file for the full stdout/stderr.",
                output_too_long_preview: "Hook output too long, showing first {n} bytes:\n{preview}",
            },
            "en",
        ),
    };
    let _ = lang;
    let json_reason = hook_json_reason(json);
    let mut sections = Vec::new();
    if let Some(reason) = non_empty_text(json_reason.as_deref().unwrap_or_default()) {
        sections.push(str::replace(labels.json_feedback, "{}", &reason));
    }
    if let Some(error) = non_empty_text(result.error.as_deref().unwrap_or_default()) {
        sections.push(str::replace(labels.stderr_error, "{}", &error));
    }
    if let Some(output) = non_empty_text(&result.output) {
        sections.push(str::replace(labels.stdout, "{}", &output));
    }
    if sections.is_empty() {
        return labels.no_reason.to_string();
    }

    let details = sections.join("\n\n");
    if details.len() <= INLINE_HOOK_OUTPUT_LIMIT {
        return details;
    }

    match write_long_hook_feedback(session_id, command, &details).await {
        Some(path) => str::replace(
            labels.output_too_long_file,
            "{}",
            &path.display().to_string(),
        ),
        None => labels
            .output_too_long_preview
            .replace("{n}", &INLINE_HOOK_OUTPUT_LIMIT.to_string())
            .replace(
                "{preview}",
                truncate_utf8(&details, INLINE_HOOK_OUTPUT_LIMIT),
            ),
    }
}

async fn write_long_hook_feedback(
    session_id: &str,
    command: &str,
    details: &str,
) -> Option<PathBuf> {
    let dir = std::env::temp_dir()
        .join("aemeath-hook-results")
        .join(session_id);
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        return None;
    }
    let file_name = format!("{}.txt", sanitized_file_stem(command));
    let path = dir.join(file_name);
    tokio::fs::write(&path, details).await.ok()?;
    Some(path)
}

fn sanitized_file_stem(command: &str) -> String {
    let mut stem: String = command
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    stem = stem.trim_matches('-').to_string();
    if stem.is_empty() {
        "hook-output".to_string()
    } else {
        stem.chars().take(80).collect()
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    &text[..text.floor_char_boundary(max_bytes)]
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

struct HookFeedbackLabels {
    json_feedback: &'static str,
    stderr_error: &'static str,
    stdout: &'static str,
    no_reason: &'static str,
    output_too_long_file: &'static str,
    output_too_long_preview: &'static str,
}

/// 检查 Stop hook 连续阻断是否超过上限。
///
/// 超过则发送 SystemMessage 提示、状态机转到 Done 并返回 `true`（调用方应 `break`）。
/// 未超过返回 `false`，调用方继续正常的阻断反馈注入与 `continue`。
pub(crate) async fn stop_hook_block_limit_reached<S>(
    block_count: usize,
    sink: &S,
    loop_fsm: &mut ChatLoopFsm,
) -> bool
where
    S: ChatEventSink,
{
    if block_count > MAX_STOP_HOOK_BLOCKS {
        sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
            "[stop hook blocked {MAX_STOP_HOOK_BLOCKS} times in a row; \
             stopping to avoid infinite loop]"
        )))
        .await;
        loop_fsm.transition(ChatLoopTransition::StopSucceeded);
        loop_fsm.assert_state(ChatLoopState::Done, "stop hook block limit exceeded");
        true
    } else {
        false
    }
}

#[cfg(test)]
#[path = "finalize_tests.rs"]
mod finalize_tests;
