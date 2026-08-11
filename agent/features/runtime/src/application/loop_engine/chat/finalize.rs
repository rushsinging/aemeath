use crate::application::loop_engine::chat::{ChatEventSink, RuntimeRunContext, RuntimeStreamEvent};
use crate::application::loop_engine::run_finalization::{
    RunFinalizationObserver, RunFinalizationOutcome,
};
use task::TaskAccess;

pub(crate) struct MainRunFinalizationObserver<'a> {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub context: &'a RuntimeRunContext,
    pub access: &'a dyn TaskAccess,
    pub session_id: &'a str,
}

#[async_trait::async_trait]
impl RunFinalizationObserver for MainRunFinalizationObserver<'_> {
    async fn on_finalized(
        &mut self,
        outcome: &RunFinalizationOutcome,
        _terminal: &tools::AgentRunTerminal,
    ) {
        crate::application::loop_engine::run_finalization::log_run_finalization(
            outcome,
            self.session_id,
        );
        let _ = self
            .sink
            .send_event(RuntimeStreamEvent::DoneWithDuration {
                context: self.context.clone(),
                duration: outcome.duration,
            })
            .await;
        if let Some(batch_id) = self.access.lifecycle_snapshot(0).all_completed {
            if let Err(error) = self.access.archive_batch(batch_id) {
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
}
