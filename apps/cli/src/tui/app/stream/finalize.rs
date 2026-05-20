use crate::agent_runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::tui::app::stream::hook_ui::HookUi;
use crate::tui::app::UiEvent;
use aemeath_core::config::hooks::HookEvent;
use aemeath_core::hook::{HookData, HookRunner, StopHookData};
use aemeath_core::task::{BatchStatus, TaskStore};
use tokio::sync::mpsc;

pub(crate) async fn finalize_main_loop(
    outcome: &AgentRunOutcome,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &HookRunner,
    session_id: &str,
    task_store: &TaskStore,
) {
    log_agent_outcome(outcome, session_id);

    match &outcome.status {
        AgentRunStatus::Completed | AgentRunStatus::MaxTurns => {
            let _ = hook_ui
                .run_plain(
                    hook_runner,
                    HookEvent::Stop,
                    None,
                    HookData::Stop(StopHookData {
                        turns: outcome.turns,
                    }),
                )
                .await;
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
        }
        AgentRunStatus::Cancelled => {
            let _ = tx.send(UiEvent::Done).await;
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
        }
    }
}
