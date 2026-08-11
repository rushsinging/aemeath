use hook::{HookInvocation, HookPort, SubRunStopInput};
use std::sync::Arc;
use tools::{AgentProgressKind, AgentProgressSourceContext};

pub use crate::application::loop_engine::run_finalization::{
    log_run_finalization as log_agent_outcome, RunFinalizationOutcome as AgentRunOutcome,
    RunFinalizationStatus as AgentRunStatus,
};

pub(crate) struct SubRunFinalizationObserver<'a> {
    pub hook_port: Arc<dyn HookPort>,
    pub activities: &'a crate::application::activity::ActivityCoordinator,
    pub run_step_id: sdk::RunStepId,
    pub workspace_root: &'a std::path::Path,
    pub session_id: &'a str,
    pub prompt: &'a str,
    pub system: &'a str,
    pub model_spec: Option<&'a str>,
    pub progress_sink: Option<&'a std::sync::Arc<dyn tools::ProgressSink>>,
    pub source_context: AgentProgressSourceContext,
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
        if let Some(sink) = self.progress_sink {
            let terminal_outcome = match terminal {
                tools::AgentRunTerminal::Completed { .. } => {
                    tools::SubRunTerminalOutcome::Completed
                }
                tools::AgentRunTerminal::Failed { error } => tools::SubRunTerminalOutcome::Failed {
                    error: error.clone(),
                },
                tools::AgentRunTerminal::Cancelled => tools::SubRunTerminalOutcome::Cancelled,
            };
            sink.emit(super::progress::build_progress_event(
                self.source_context.clone(),
                outcome.run_steps.saturating_mul(1024).saturating_add(1023),
                AgentProgressKind::Terminal {
                    outcome: terminal_outcome,
                },
            ));
        }
        let is_error = matches!(
            outcome.status,
            AgentRunStatus::Cancelled
                | AgentRunStatus::Failed(_)
                | AgentRunStatus::TimedOut
                | AgentRunStatus::ApiError(_)
        );
        let outcome_dispatch = crate::application::loop_engine::chat::hook_ui::dispatch_hook(
            &self.hook_port,
            self.activities,
            &self.run_step_id,
            HookInvocation::SubRunStop(SubRunStopInput {
                prompt: self.prompt.to_string(),
                system: self.system.to_string(),
                model_spec: self.model_spec.map(str::to_string),
                result: terminal.output(),
                run_steps: outcome.run_steps,
                is_error,
            }),
            self.workspace_root,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
        for msg in &outcome_dispatch.messages {
            if let crate::application::hook::outcome_mapper::RuntimeHookDisplayMessageKind::SystemMessage = msg.kind {
                if let Some(sink) = self.progress_sink {
                    sink.emit(super::progress::build_progress_event(
                        self.source_context.clone(),
                        outcome.run_steps,
                        AgentProgressKind::Message {
                            text: format!("[hook] {}", msg.text),
                        },
                    ));
                }
            }
        }
    }
}
