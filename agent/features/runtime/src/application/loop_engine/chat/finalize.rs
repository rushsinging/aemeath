use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::run::derived::AgentRunOutcome;
use task::TaskAccess;

pub(crate) async fn finish_completed_loop(
    outcome: &AgentRunOutcome,
    sink: &crate::application::loop_engine::chat::ChatEventSinkHandle,
    context: &RuntimeTurnContext,
    access: &dyn TaskAccess,
) {
    let _ = sink
        .send_event(RuntimeStreamEvent::DoneWithDuration {
            context: context.clone(),
            duration: outcome.duration,
        })
        .await;

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
