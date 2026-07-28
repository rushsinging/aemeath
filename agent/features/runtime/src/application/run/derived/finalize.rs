use hook::{HookDispatchContext, HookInvocation, HookPort, SubRunStopInput};
use std::sync::Arc;
use tools::{AgentProgressEvent, AgentProgressKind};

pub use crate::application::loop_engine::run_finalization::{
    log_run_finalization as log_agent_outcome, RunFinalizationOutcome as AgentRunOutcome,
    RunFinalizationStatus as AgentRunStatus,
};

pub(crate) struct SubRunFinalizationObserver<'a> {
    pub hook_port: Arc<dyn HookPort>,
    pub workspace_root: &'a std::path::Path,
    pub session_id: &'a str,
    pub prompt: &'a str,
    pub system: &'a str,
    pub model_spec: Option<&'a str>,
    pub progress_sink: Option<&'a std::sync::Arc<dyn tools::ProgressSink>>,
}

#[async_trait::async_trait]
impl crate::application::loop_engine::run_finalization::RunFinalizationObserver
    for SubRunFinalizationObserver<'_>
{
    async fn on_finalized(
        &mut self,
        outcome: &AgentRunOutcome,
        terminal: &tools::AgentRunTerminal,
    ) {
        log_agent_outcome(outcome, self.session_id);
        let is_error = matches!(
            outcome.status,
            AgentRunStatus::Cancelled
                | AgentRunStatus::Failed(_)
                | AgentRunStatus::TimedOut
                | AgentRunStatus::ApiError(_)
        );
        let outcome_dispatch = self
            .hook_port
            .dispatch_at(
                HookInvocation::SubRunStop(SubRunStopInput {
                    prompt: self.prompt.to_string(),
                    system: self.system.to_string(),
                    model_spec: self.model_spec.map(str::to_string),
                    result: terminal.output(),
                    turns: outcome.turns,
                    is_error,
                }),
                HookDispatchContext::new(self.workspace_root),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await;
        for msg in &outcome_dispatch.messages {
            if let hook::HookDisplayMessageKind::SystemMessage = msg.kind {
                if let Some(sink) = self.progress_sink {
                    sink.emit(AgentProgressEvent {
                        sequence: outcome.turns,
                        kind: AgentProgressKind::Message {
                            text: format!("[hook] {}", msg.text),
                        },
                    });
                }
            }
        }
    }
}
