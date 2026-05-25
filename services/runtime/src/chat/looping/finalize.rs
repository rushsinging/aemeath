use crate::api::agent_runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::api::core::config::hooks::HookEvent;
use crate::api::hook::hook::{HookData, HookJsonOutput, HookResult, HookRunner, StopHookData};
use crate::api::core::task::{BatchStatus, TaskStore};
use crate::chat::looping::hook_ui::HookUi;
use crate::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use std::path::PathBuf;

const INLINE_HOOK_OUTPUT_LIMIT: usize = 4_000;

/// Run stop/failure hooks and return feedback if the loop should continue.
///
/// **Bug #49 note**: input queue draining happens *before* this function is
/// called (in `stream.rs`). If queued input exists, the loop `continue`s
/// without reaching here. When a stop hook blocks the stop, the returned
/// feedback is injected as a system-reminder and the loop `continue`s — the
/// next iteration will again drain the queue first.
pub(crate) async fn finalize_main_loop<S>(
    outcome: &AgentRunOutcome,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &HookRunner,
    session_id: &str,
    task_store: &TaskStore,
) -> Option<String>
where
    S: ChatEventSink,
{
    log_agent_outcome(outcome, session_id);

    match &outcome.status {
        AgentRunStatus::Completed | AgentRunStatus::MaxTurns => {
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
            if let Some(feedback) = stop_hook_feedback(&stop_results, session_id).await {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(feedback.clone()))
                    .await;
                return Some(feedback);
            }

            let _ = sink
                .send_event(RuntimeStreamEvent::DoneWithDuration(outcome.duration))
                .await;

            if let Some(active) = task_store.active_list().await {
                if task_store.is_batch_completed(active.id).await {
                    task_store
                        .set_batch_status(active.id, BatchStatus::Archived)
                        .await;
                    log::info!(
                        "[task_list_archived] batch_id={}, status=archived, reason=all_tasks_completed",
                        active.id
                    );
                }
            }
            None
        }
        AgentRunStatus::Cancelled => {
            let _ = sink.send_event(RuntimeStreamEvent::Done).await;
            None
        }
        AgentRunStatus::ApiError(_) | AgentRunStatus::TimedOut => {
            let stop_results = hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::StopFailure,
                    None,
                    HookData::Stop(StopHookData {
                        turns: outcome.turns,
                    }),
                )
                .await;
            let (system_message, additional_context) = stop_results
                .into_iter()
                .find_map(|(_, _, json_output)| json_output)
                .map(|output| (output.system_message, output.additional_context))
                .unwrap_or((None, None));
            let _ = sink
                .send_event(RuntimeStreamEvent::StopFailureHook {
                    system_message,
                    additional_context,
                })
                .await;
            let _ = sink.send_event(RuntimeStreamEvent::Done).await;
            None
        }
    }
}

async fn stop_hook_feedback(
    hook_results: &[(
        crate::api::core::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
    session_id: &str,
) -> Option<String> {
    let (entry, result, json) = hook_results
        .iter()
        .filter(|(_, result, json)| is_stop_blocked(result, json))
        .find(|(_, result, json)| has_hook_feedback(result, json))
        .or_else(|| {
            hook_results
                .iter()
                .find(|(_, result, json)| is_stop_blocked(result, json))
        })?;
    let details = hook_feedback_details(result, json, session_id, &entry.command).await;
    Some(format!(
        "Stop hook 阻止了停止，请先解决以下问题后再结束：\n命令：{}\n{}",
        entry.command, details
    ))
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

async fn hook_feedback_details(
    result: &HookResult,
    json: &Option<HookJsonOutput>,
    session_id: &str,
    command: &str,
) -> String {
    let json_reason = hook_json_reason(json);
    let mut sections = Vec::new();
    if let Some(reason) = non_empty_text(json_reason.as_deref().unwrap_or_default()) {
        sections.push(format!("JSON 反馈：\n{}", reason));
    }
    if let Some(error) = non_empty_text(result.error.as_deref().unwrap_or_default()) {
        sections.push(format!("stderr/错误：\n{}", error));
    }
    if let Some(output) = non_empty_text(&result.output) {
        sections.push(format!("stdout：\n{}", output));
    }
    if sections.is_empty() {
        return "Stop hook 阻止了停止，但没有提供原因".to_string();
    }

    let details = sections.join("\n\n");
    if details.len() <= INLINE_HOOK_OUTPUT_LIMIT {
        return details;
    }

    match write_long_hook_feedback(session_id, command, &details).await {
        Some(path) => format!(
            "hook 输出过长，已保存到文件：{}\n请读取该文件查看完整 stdout/stderr。",
            path.display()
        ),
        None => format!(
            "hook 输出过长，以下为前 {} 字节预览：\n{}",
            INLINE_HOOK_OUTPUT_LIMIT,
            truncate_utf8(&details, INLINE_HOOK_OUTPUT_LIMIT)
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

#[cfg(test)]
fn stop_hook_feedback_for_test(
    hook_results: &[(
        crate::api::core::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
) -> Option<String> {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(stop_hook_feedback(hook_results, "test-session"))
}

#[cfg(test)]
fn hook_result(
    command: &str,
    blocked: bool,
    output: &str,
    error: Option<&str>,
) -> (
    crate::api::core::config::hooks::HookEntry,
    HookResult,
    Option<HookJsonOutput>,
) {
    (
        crate::api::core::config::hooks::HookEntry {
            matcher: String::new(),
            command: command.to_string(),
            timeout: 60,
        },
        HookResult {
            blocked,
            output: output.to_string(),
            error: error.map(str::to_string),
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::config::hooks::HookEntry;

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
        };

        let feedback = runtime.block_on(hook_feedback_details(
            &result,
            &None,
            "test-long-output",
            "check long.sh",
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
            },
            Some(HookJsonOutput {
                decision: Some("block".to_string()),
                reason: Some(reason.to_string()),
                ..Default::default()
            }),
        )
    }
}
