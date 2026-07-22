use crate::application::agent::runner::AgentRunOutcome;
use crate::application::chat::looping::hook_ui::dispatch_hook;
use crate::application::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use crate::application::hook_adapter::{
    RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookReason,
};
use hook::{HookInvocation, HookPort, StopInput};
use share::message::StopHookFeedback;
use std::path::PathBuf;
use std::sync::Arc;
use task::TaskAccess;
use tokio_util::sync::CancellationToken;

const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;
const TUI_HOOK_OUTPUT_PREVIEW_LIMIT: usize = 1_000;

pub(crate) struct StopHookFeedbackMessage {
    pub llm_text: String,
    pub payload: StopHookFeedback,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_stop_hook_before_finish<S>(
    outcome: &AgentRunOutcome,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    session_id: &str,
    language: &str,
    workspace_root: &std::path::Path,
    cancel: &CancellationToken,
) -> Option<StopHookFeedbackMessage>
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
) -> StopHookFeedbackMessage {
    let detail = dispatch
        .block_detail
        .as_ref()
        .expect("Stop hook Block must carry the blocking subscription detail");
    let command = detail.command.clone();
    let reason = format_reason(&dispatch.directive);
    let stdout_preview =
        truncate_utf8(&detail.execution.stdout, TUI_HOOK_OUTPUT_PREVIEW_LIMIT).to_string();
    let stderr_preview =
        truncate_utf8(&detail.execution.stderr, TUI_HOOK_OUTPUT_PREVIEW_LIMIT).to_string();
    let stdout_truncated = detail.execution.stdout.len() > stdout_preview.len();
    let stderr_truncated = detail.execution.stderr.len() > stderr_preview.len();
    let output = format!(
        "command: {command}\nexit_code: {:?}\nreason: {reason}\n\nstdout:\n{}\n\nstderr:\n{}",
        detail.execution.exit_code, detail.execution.stdout, detail.execution.stderr
    );
    let output_file = if output.len() > INLINE_HOOK_OUTPUT_LIMIT {
        write_long_hook_feedback(session_id, &command, &output)
            .await
            .map(|path| path.display().to_string())
    } else {
        None
    };
    let summary = match language {
        "zh" => "Stop hook 阻止了停止。".to_string(),
        _ => "Stop hook prevented stopping.".to_string(),
    };
    let payload = StopHookFeedback {
        summary: summary.clone(),
        command,
        exit_code: detail.execution.exit_code,
        reason,
        stdout_preview,
        stderr_preview,
        stdout_truncated,
        stderr_truncated,
        output_file,
    };
    let llm_text = stop_hook_llm_text(&payload, language);

    StopHookFeedbackMessage { llm_text, payload }
}

fn format_reason(directive: &RuntimeHookDirective) -> String {
    match directive {
        RuntimeHookDirective::Block { reason } => match reason {
            RuntimeHookReason::ExitCode { code, .. } => format!("exit code {code}"),
            RuntimeHookReason::JsonBlock { reason } => reason.clone(),
            RuntimeHookReason::JsonContinueFalse { stop_reason } => stop_reason
                .clone()
                .unwrap_or_else(|| "hook returned continue:false".to_string()),
            RuntimeHookReason::StopHookExecutionFailed { error }
            | RuntimeHookReason::PolicyBlock { error } => error.clone(),
        },
        _ => "hook blocked completion".to_string(),
    }
}

fn stop_hook_llm_text(payload: &StopHookFeedback, language: &str) -> String {
    let mut text = format!(
        "{}\nCommand: {}\nExit code: {}\nReason: {}",
        payload.summary,
        payload.command,
        payload
            .exit_code
            .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
        payload.reason
    );
    if let Some(path) = &payload.output_file {
        let instruction = match language {
            "zh" => format!("\n完整 hook 输出已保存到 {path}；请使用 Read 工具查看。"),
            _ => format!("\nFull hook output is saved to {path}; use the Read tool to inspect it."),
        };
        text.push_str(&instruction);
    } else {
        if !payload.stderr_preview.trim().is_empty() {
            text.push_str(&format!("\nstderr:\n{}", payload.stderr_preview));
        }
        if !payload.stdout_preview.trim().is_empty() {
            text.push_str(&format!("\nstdout:\n{}", payload.stdout_preview));
        }
    }
    text
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

#[cfg(test)]
#[path = "finalize_tests.rs"]
mod finalize_tests;
