use crate::business::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::business::chat::looping::hook_ui::{runtime_hook_event_finished, HookUi};
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use hook::api::{HookData, HookJsonOutput, HookResult, HookRunner, StopHookData};
use share::config::hooks::HookEvent;
use std::path::PathBuf;
use storage::api::{BatchStatus, TaskStore};

const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;

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
            hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::StopFailure,
                    None,
                    HookData::Stop(StopHookData {
                        turns: outcome.turns,
                    }),
                )
                .await;
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
    sink: &S,
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
        if let Some((entry, result, json_output)) = stop_hook_blocking_result(&stop_results) {
            let _ = sink
                .send_event(RuntimeStreamEvent::HookEvent(runtime_hook_event_finished(
                    "Stop",
                    entry,
                    result,
                    json_output,
                )))
                .await;
        }
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
            log::info!(target: "runtime::finalize",
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
        .filter(|(_, result, json)| is_stop_blocked(result, json))
        .find(|(_, result, json)| has_hook_feedback(result, json))
        .or_else(|| {
            hook_results
                .iter()
                .find(|(_, result, json)| is_stop_blocked(result, json))
        })
        .map(|(entry, result, json)| (entry, result, json))
}

fn is_stop_blocked(result: &HookResult, json: &Option<HookJsonOutput>) -> bool {
    result.blocked
        || json
            .as_ref()
            .is_some_and(|j| j.decision.as_deref() == Some("block"))
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
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end] // allow unsafe_text_op: end is adjusted to UTF-8 char boundary
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
fn stop_hook_feedback_for_test(
    hook_results: &[(
        share::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
) -> Option<String> {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(stop_hook_feedback(hook_results, "test-session", "zh"))
}

#[cfg(test)]
fn hook_result(
    command: &str,
    blocked: bool,
    output: &str,
    error: Option<&str>,
) -> (
    share::config::hooks::HookEntry,
    HookResult,
    Option<HookJsonOutput>,
) {
    (
        share::config::hooks::HookEntry {
            matcher: String::new(),
            command: command.to_string(),
            timeout: 60,
        },
        HookResult {
            blocked,
            output: output.to_string(),
            error: error.map(str::to_string),
            exit_code: if blocked { Some(2) } else { Some(0) },
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::hooks::HookEntry;

    #[test]
    fn test_stop_hook_feedback_returns_none_without_block() {
        let results = vec![hook_result("echo ok", false, "done", None)];

        assert!(stop_hook_feedback_for_test(&results).is_none());
    }

    #[test]
    fn test_stop_hook_feedback_uses_error_when_blocked() {
        let results = vec![hook_result("check.sh", true, "", Some("failed"))];

        let feedback = stop_hook_feedback_for_test(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("failed"));
    }

    #[test]
    fn test_stop_hook_feedback_uses_stdout_when_blocked() {
        let results = vec![hook_result("check.sh", true, "unsafe op found\n", None)];

        let feedback = stop_hook_feedback_for_test(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("unsafe op found"));
    }

    #[test]
    fn test_stop_hook_feedback_uses_later_stdout_after_empty_blocked_result() {
        let results = vec![
            hook_result("build.sh", false, "build ok", None),
            hook_result("line-check.sh", true, "", None),
            hook_result("line-check.sh", true, "line limit exceeded", None),
        ];

        let feedback = stop_hook_feedback_for_test(&results).unwrap();

        assert!(feedback.contains("line-check.sh"));
        assert!(feedback.contains("line limit exceeded"));
    }

    #[test]
    fn test_stop_hook_feedback_includes_error_and_stdout_when_blocked() {
        let results = vec![hook_result(
            "check.sh",
            true,
            "stdout details",
            Some("stderr details"),
        )];

        let feedback = stop_hook_feedback_for_test(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("stderr/错误"));
        assert!(feedback.contains("stderr details"));
        assert!(feedback.contains("stdout："));
        assert!(feedback.contains("stdout details"));
    }

    #[test]
    fn test_hook_feedback_details_writes_long_output_to_file() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let long_output = "x".repeat(INLINE_HOOK_OUTPUT_LIMIT + 1);
        let result = HookResult {
            blocked: true,
            output: long_output,
            error: Some("stderr details".to_string()),
            exit_code: Some(2),
        };

        let feedback = runtime.block_on(hook_feedback_details(
            &result,
            &None,
            "test-long-output",
            "check long.sh",
            "zh",
        ));

        assert!(feedback.contains("hook 输出过长"));
        assert!(feedback.contains("已保存到文件"));
        assert!(feedback.contains(&std::env::temp_dir().display().to_string()));
    }

    #[test]
    fn test_stop_hook_feedback_uses_json_reason() {
        let results = vec![hook_result_with_json_reason("check.sh", "fix line count")];

        let feedback = stop_hook_feedback_for_test(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("fix line count"));
    }

    #[test]
    fn test_stop_hook_feedback_tells_llm_it_must_not_finish() {
        let results = vec![hook_result(
            "check-stop.sh",
            true,
            "fix the failing test",
            Some("exit code 2"),
        )];

        let feedback = stop_hook_feedback_for_test(&results).unwrap();

        assert!(
            feedback.contains("不能结束") || feedback.contains("MUST NOT finish"),
            "feedback must explicitly tell the LLM it cannot finish yet: {feedback}"
        );
        assert!(
            feedback.contains("MUST") || feedback.contains("必须"),
            "feedback must use mandatory language: {feedback}"
        );
        assert!(feedback.contains("check-stop.sh"));
        assert!(feedback.contains("fix the failing test"));
    }

    fn hook_result_with_json_reason(
        command: &str,
        reason: &str,
    ) -> (HookEntry, HookResult, Option<HookJsonOutput>) {
        (
            HookEntry {
                matcher: String::new(),
                command: command.to_string(),
                timeout: 60,
            },
            HookResult {
                blocked: false,
                output: String::new(),
                error: None,
                exit_code: Some(0),
            },
            Some(HookJsonOutput {
                decision: Some("block".to_string()),
                reason: Some(reason.to_string()),
                ..Default::default()
            }),
        )
    }
}
