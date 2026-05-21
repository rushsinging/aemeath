use crate::agent_runner::{AgentRunOutcome, AgentRunStatus, log_agent_outcome};
use crate::tui::app::UiEvent;
use crate::tui::app::stream::hook_ui::HookUi;
use aemeath_core::config::hooks::HookEvent;
use aemeath_core::hook::{HookData, HookJsonOutput, HookResult, HookRunner, StopHookData};
use aemeath_core::task::{BatchStatus, TaskStore};
use tokio::sync::mpsc;

pub(crate) async fn finalize_main_loop(
    outcome: &AgentRunOutcome,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &HookRunner,
    session_id: &str,
    task_store: &TaskStore,
) -> Option<String> {
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
            if let Some(feedback) = stop_hook_feedback(&stop_results) {
                let _ = tx.send(UiEvent::SystemMessage(feedback.clone())).await;
                return Some(feedback);
            }

            let _ = tx.send(UiEvent::DoneWithDuration(outcome.duration)).await;

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
            let _ = tx.send(UiEvent::Done).await;
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
            let _ = tx
                .send(UiEvent::StopFailureHook {
                    system_message,
                    additional_context,
                })
                .await;
            let _ = tx.send(UiEvent::Done).await;
            None
        }
    }
}

fn stop_hook_feedback(
    hook_results: &[(
        aemeath_core::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )],
) -> Option<String> {
    hook_results
        .iter()
        .find(|(_, result, json)| {
            result.blocked
                || json
                    .as_ref()
                    .is_some_and(|j| j.decision.as_deref() == Some("block"))
        })
        .map(|(entry, result, json)| {
            let reason = json
                .as_ref()
                .and_then(|j| {
                    j.reason
                        .clone()
                        .or_else(|| j.system_message.clone())
                        .or_else(|| j.additional_context.clone())
                        .or_else(|| j.stop_reason.clone())
                })
                .or_else(|| result.error.clone())
                .or_else(|| non_empty_text(&result.output))
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| "Stop hook 阻止了停止，但没有提供原因".to_string());
            format!(
                "Stop hook 阻止了停止，请先解决以下问题后再结束：\n命令：{}\n{}",
                entry.command, reason
            )
        })
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
mod tests {
    use super::*;
    use aemeath_core::config::hooks::HookEntry;

    #[test]
    fn test_stop_hook_feedback_returns_none_without_block() {
        let results = vec![hook_result("echo ok", false, "done", None)];

        assert!(stop_hook_feedback(&results).is_none());
    }

    #[test]
    fn test_stop_hook_feedback_uses_error_when_blocked() {
        let results = vec![hook_result("check.sh", true, "", Some("failed"))];

        let feedback = stop_hook_feedback(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("failed"));
    }

    #[test]
    fn test_stop_hook_feedback_uses_stdout_when_blocked() {
        let results = vec![hook_result("check.sh", true, "unsafe op found\n", None)];

        let feedback = stop_hook_feedback(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("unsafe op found"));
    }

    #[test]
    fn test_stop_hook_feedback_uses_json_reason() {
        let results = vec![hook_result_with_json_reason("check.sh", "fix line count")];

        let feedback = stop_hook_feedback(&results).unwrap();

        assert!(feedback.contains("check.sh"));
        assert!(feedback.contains("fix line count"));
    }

    fn hook_result(
        command: &str,
        blocked: bool,
        output: &str,
        error: Option<&str>,
    ) -> (HookEntry, HookResult, Option<HookJsonOutput>) {
        (
            HookEntry {
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
