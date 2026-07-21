use crate::application::agent::runner::AgentRunOutcome;
use crate::application::chat::looping::hook_ui::dispatch_hook;
use crate::application::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use crate::application::hook_adapter::{RuntimeHookDirective, RuntimeHookDispatch};
use hook::{HookInvocation, HookPort, StopInput};
use std::path::PathBuf;
use std::sync::Arc;
use task::TaskAccess;
use tokio_util::sync::CancellationToken;

const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_stop_hook_before_finish<S>(
    outcome: &AgentRunOutcome,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    session_id: &str,
    language: &str,
    workspace_root: &std::path::Path,
    cancel: &CancellationToken,
) -> Option<String>
where
    S: ChatEventSink,
{
    let dispatch = dispatch_hook(
        hook_port,
        sink,
        HookInvocation::Stop(StopInput {
            turns: outcome.turns,
        }),
        workspace_root,
        cancel,
    )
    .await;
    if matches!(dispatch.directive, RuntimeHookDirective::Block { .. }) {
        let feedback = stop_hook_feedback(&dispatch, session_id, language).await;
        return Some(feedback);
    }
    None
}

pub(crate) async fn finish_completed_loop<S>(
    outcome: &AgentRunOutcome,
    sink: &S,
    context: &RuntimeTurnContext,
    access: &dyn TaskAccess,
) where
    S: ChatEventSink,
{
    let _ = sink
        .send_event(RuntimeStreamEvent::DoneWithDuration {
            context: context.clone(),
            duration: outcome.duration,
        })
        .await;

    // #889：当 current batch 的全部任务完成时归档它。`all_completed` 与
    // stale 阈值无关，此处传入 `0` 仅为满足 lifecycle_snapshot 签名。
    if let Some(batch_id) = access.lifecycle_snapshot(0).all_completed {
        if let Err(error) = access.archive_batch(batch_id) {
            log::warn!(target: crate::LOG_TARGET,
                "[task_list_archive_failed] batch_id={batch_id}, error={error}"
            );
        } else {
            log::info!(target: crate::LOG_TARGET,
                "[task_list_archived] batch_id={batch_id}, status=archived, reason=all_tasks_completed"
            );
        }
    }
}

async fn stop_hook_feedback(
    dispatch: &RuntimeHookDispatch,
    session_id: &str,
    language: &str,
) -> String {
    log::debug!(target: crate::LOG_TARGET,
        "[stop_hook_debug] session={} stop hook directive={:?} executions={} messages={}",
        session_id,
        dispatch.directive,
        dispatch.executions.len(),
        dispatch.messages.len(),
    );

    // Extract stdout, stderr from the last execution
    let last = dispatch.executions.last();
    let stdout = last.map(|e| e.stdout.as_str()).unwrap_or("");
    let stderr = last.map(|e| e.stderr.as_str()).unwrap_or("");

    let source = dispatch
        .messages
        .first()
        .map(|m| m.source.as_str())
        .unwrap_or("stop hook");

    let details = stop_hook_feedback_details(
        stdout,
        stderr,
        &dispatch.messages,
        session_id,
        source,
        language,
    )
    .await;

    let template = match language {
        "zh" => "Stop hook 阻止了停止。你现在还不能结束本轮处理。\n你 MUST 先满足下面 Stop hook 的要求，然后才能再次尝试停止。\n命令：{cmd}\n{details}",
        _ => "Stop hook prevented stopping. You cannot finish this turn yet.\nYou MUST first satisfy the Stop hook requirement below, then attempt to stop again.\nCommand: {cmd}\n{details}",
    };
    template
        .replace("{cmd}", source)
        .replace("{details}", &details)
}

async fn stop_hook_feedback_details(
    stdout: &str,
    stderr: &str,
    messages: &[crate::application::hook_adapter::RuntimeHookDisplayMessage],
    session_id: &str,
    command: &str,
    language: &str,
) -> String {
    let (labels, _lang) = match language {
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
    let _ = _lang;

    // Collect system messages from display messages
    let sys_msgs: Vec<&str> = messages
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                crate::application::hook_adapter::RuntimeHookDisplayMessageKind::SystemMessage
            )
        })
        .map(|m| m.text.as_str())
        .collect();

    let mut sections = Vec::new();

    // System messages carry reason/feedback
    for msg in &sys_msgs {
        sections.push(str::replace(labels.json_feedback, "{}", msg));
    }

    if let Some(error) = non_empty_text(stderr) {
        sections.push(str::replace(labels.stderr_error, "{}", &error));
    }
    if let Some(output) = non_empty_text(stdout) {
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

#[cfg(test)]
#[path = "finalize_tests.rs"]
mod finalize_tests;
