use super::progress::build_tool_calls_progress_event;

use std::sync::Arc;

use async_trait::async_trait;
use share::message::Message;
use share::string_idx::slice_head;
use tokio_util::sync::CancellationToken;
use tools::{AgentProgressKind, AgentProgressSourceContext, AgentRunTerminal};

use crate::application::loop_engine::chat::InvocationResponse;
use crate::application::loop_engine::event_strategy::{ProgressTerminalObserver, RunEventObserver};
use crate::application::loop_engine::{EventSinkPort, LoopEngineError, ModelStep};
use crate::application::model::invocation::{ModelInvocationObserver, ModelInvocationSource};
use crate::application::run::context::RuntimeContext;
use crate::application::run::creation::RunInstance;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::Agent;
use crate::domain::agent_run::RunDomainEvent;
use crate::ports::StopReason;

pub(super) type ProgressReporter = Arc<dyn Fn(Option<usize>, &str) + Send + Sync>;

pub(super) fn sub_run_log_context(
    parent: &logging::LogContext,
    session_id: &str,
    sub_run_id: &str,
    model: &str,
    provider: &str,
    role: &str,
) -> logging::LogContext {
    parent.patched(logging::LogContextPatch {
        session_id: logging::FieldPatch::Set(session_id.to_string()),
        chat_id: logging::FieldPatch::Set(sub_run_id.to_string()),
        turn: logging::FieldPatch::Clear,
        request_id: logging::FieldPatch::Clear,
        model: logging::FieldPatch::Set(model.to_string()),
        provider: logging::FieldPatch::Set(provider.to_string()),
        role: logging::FieldPatch::Set(role.to_string()),
    })
}

pub(super) fn sub_request_log_context(
    parent: &logging::LogContext,
    model: &str,
    provider: &str,
    role: &str,
) -> logging::LogContext {
    parent.patched(logging::LogContextPatch {
        request_id: logging::FieldPatch::Set(uuid::Uuid::now_v7().to_string()),
        model: logging::FieldPatch::Set(model.to_string()),
        provider: logging::FieldPatch::Set(provider.to_string()),
        role: logging::FieldPatch::Set(role.to_string()),
        ..logging::LogContextPatch::default()
    })
}

pub(super) struct CancellationPropagationGuard(tokio::task::JoinHandle<()>);

impl CancellationPropagationGuard {
    pub(super) fn new(
        signal: Arc<dyn tools::CancellationSignal>,
        token: CancellationToken,
    ) -> Self {
        Self(tokio::spawn(async move {
            signal.cancelled().await;
            token.cancel();
        }))
    }
}

impl Drop for CancellationPropagationGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn launch_sub_run(
    instance: &mut RunInstance,
    active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    tool_execution_context: tools::ToolExecutionContext,
    mut input: crate::application::loop_engine::input_strategy::FixedInputAdapter<'_>,
    mut events: DerivedEventPort,
    mut model: crate::application::loop_engine::run_services::RuntimeModelInvocation<
        DerivedModelObserver,
    >,
    mut persistence: crate::application::loop_engine::run_services::RuntimeStepPersistence<
        '_,
        crate::application::loop_engine::step_persistence::NoopAcceptedInputObserver,
    >,
    mut compaction: crate::application::loop_engine::run_services::RuntimeCompaction<
        '_,
        crate::application::loop_engine::compaction::NoopCompactionObserver,
    >,
    mut interaction: crate::application::loop_engine::run_services::RuntimeInteraction<
        crate::application::loop_engine::run_services::ProgressInteractionPublisher<'_>,
    >,
    mut stop_hook: crate::application::loop_engine::run_services::RuntimeStopHook<
        crate::application::hook::stop_coordination::NoopStopHookObserver,
    >,
    mut tools: crate::application::loop_engine::run_services::RuntimeToolOrchestration<
        '_,
        ProgressToolRoundObserver,
    >,
    mut stuck: DerivedStuckObserver,
    plan_mode: bool,
    finalizer: SubRunFinalizer,
) -> AgentRunTerminal {
    let runtime_context = instance.context().clone();
    let cancel = runtime_context.cancel().token().clone();
    let _signal_propagation =
        CancellationPropagationGuard::new(tool_execution_context.cancellation(), cancel.clone());
    let control = crate::application::loop_engine::run_ports::NoopRunControl;
    let lifecycle_active_run = active_run.clone();
    let lifecycle = crate::application::loop_engine::run_ports::ActiveRunLifecycle::new(
        lifecycle_active_run.as_ref(),
        crate::application::loop_engine::run_ports::StepScopeRegistration::Disabled,
    );
    let plan_approval =
        crate::application::loop_engine::run_ports::FixedPlanApproval::new(plan_mode);
    let mut loop_context = crate::application::loop_engine::RunLoop::new(
        &mut input,
        &mut events,
        &control,
        &lifecycle,
        &mut interaction,
        &mut persistence,
        &mut compaction,
        &mut model,
        &mut stop_hook,
        &mut tools,
        &mut stuck,
        &plan_approval,
    );
    let launch_result =
        crate::application::run::launcher::launch(instance, cancel, active_run, &mut loop_context)
            .await;
    finalizer
        .finish(instance.execution_mut(), launch_result)
        .await
}

pub(super) struct DerivedEventPort {
    pub progress: ProgressReporter,
}

#[async_trait]
impl EventSinkPort for DerivedEventPort {
    async fn emit(
        &mut self,
        execution: &mut RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        let turn_count = execution.turn_count();
        ProgressTerminalObserver {
            progress: self.progress.as_ref(),
            terminal: execution.terminal_mut(),
            turn_count,
        }
        .emit(events)
        .await
    }
}

pub(super) struct DerivedModelObserver {
    pub runtime_context: RuntimeContext,
    pub progress_sink: Option<Arc<dyn tools::ProgressSink>>,
    pub source_context: AgentProgressSourceContext,
    pub runtime_cancellation: CancellationToken,
    pub role_name: String,
    pub model_name: String,
    pub context_size: usize,
    pub progress: ProgressReporter,
}

impl DerivedModelObserver {
    fn progress_turn_start(&self, execution: &RunExecutionState) {
        (self.progress)(
            Some(execution.turn_count()),
            &format!(
                "Agent turn {}, messages: {}, est_tokens: {}",
                execution.turn_count(),
                execution.messages_len(),
                execution.message_tokens(),
            ),
        );
    }

    fn progress_api_ok(&self, turn: usize, response: &InvocationResponse) {
        (self.progress)(
            Some(turn),
            &format!(
                "API ok: in={} out={} stop={:?}",
                response.usage.input_tokens.unwrap_or(0),
                response.usage.output_tokens.unwrap_or(0),
                response.stop_reason,
            ),
        );
    }

    fn send_text_progress(&self, turn: usize, response: &InvocationResponse) {
        let Some(sink) = self.progress_sink.as_ref() else {
            return;
        };
        let text = response.assistant_message.text_content();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let text = if trimmed.len() > 300 {
            format!("{}...", slice_head(trimmed, 300))
        } else {
            trimmed.to_string()
        };
        sink.emit(super::progress::build_progress_event(
            self.source_context.clone(),
            turn,
            AgentProgressKind::Message { text },
        ));
    }
}

impl ModelInvocationSource for DerivedModelObserver {
    fn runtime_context(&self) -> &RuntimeContext {
        &self.runtime_context
    }

    fn role(&self) -> &str {
        &self.role_name
    }

    fn request_log_context(&self, parent: &logging::LogContext) -> logging::LogContext {
        sub_request_log_context(
            parent,
            &self.model_name,
            &self.runtime_context.provider_ref().model.provider,
            &self.role_name,
        )
    }

    fn context_size(&self, _execution: &RunExecutionState) -> usize {
        self.context_size
    }

    fn committed_delta(&self) -> bool {
        false
    }

    fn build_reducer(
        &self,
    ) -> crate::application::loop_engine::chat::InvocationEventReducer<
        crate::application::loop_engine::chat::ChatEventSinkHandle,
    > {
        crate::application::loop_engine::chat::InvocationEventReducer::new(
            self.runtime_context.event_sink(),
        )
    }

    fn extract_tool_calls(
        &self,
        response: &InvocationResponse,
    ) -> Vec<crate::application::tool::agent::ToolCall> {
        Agent::extract_tool_calls(&response.assistant_message)
    }
}

#[async_trait]
impl ModelInvocationObserver for DerivedModelObserver {
    async fn on_window(&mut self, execution: &RunExecutionState) {
        self.progress_turn_start(execution);
    }

    async fn on_retry(&mut self, attempt: u32, delay: std::time::Duration) {
        log::info!(
            target: crate::LOG_TARGET,
            "derived model invocation retrying: attempt={} delay_ms={}",
            attempt,
            delay.as_millis(),
        );
    }

    async fn on_retry_cancelled(&mut self) {
        self.runtime_cancellation.cancel();
    }

    async fn on_response(
        &mut self,
        execution: &mut RunExecutionState,
        response: &InvocationResponse,
        _elapsed_secs: f64,
    ) {
        self.progress_api_ok(execution.turn_count(), response);
        self.send_text_progress(execution.turn_count(), response);
    }

    async fn classify_terminal(
        &mut self,
        execution: &mut RunExecutionState,
        response: &InvocationResponse,
        tool_calls: Vec<crate::application::tool::agent::ToolCall>,
        usage: crate::application::loop_engine::StepTokenUsage,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        if response.stop_reason == StopReason::MaxOutputTokens {
            log::warn!(
                target: crate::LOG_TARGET,
                "turn {}: 模型响应触发 max_tokens 限制，注入分块提示",
                execution.turn_count(),
            );
            execution.append_message(Message::user(
                "[系统提示] 你的上一次响应触达了 max_tokens 限制，输出被截断。\
                 请基于已有内容继续，或用更紧凑的方式重新组织响应：\
                 大文件改用 Edit 分块写入（每次 < 12k 字符），\
                 长命令用 Bash heredoc 分段执行。\
                 不要重复已输出的内容，直接从截断点继续。"
                    .to_string(),
            ));
            if tool_calls.is_empty() {
                self.runtime_context.usage().reset();
                return Ok((
                    ModelStep::Tools {
                        text: response.assistant_message.text_content(),
                        calls: Vec::new(),
                    },
                    usage,
                ));
            }
        }
        if tool_calls.is_empty() {
            return Ok((
                ModelStep::Complete {
                    text: response.assistant_message.text_content(),
                },
                usage,
            ));
        }
        Ok((
            ModelStep::Tools {
                text: response.assistant_message.text_content(),
                calls: tool_calls,
            },
            usage,
        ))
    }
}

pub(super) struct ProgressToolRoundObserver {
    pub progress_sink: Option<Arc<dyn tools::ProgressSink>>,
    pub source_context: AgentProgressSourceContext,
    pub progress: ProgressReporter,
    pub role_name: String,
}

#[async_trait]
impl crate::application::tool::coordination::ToolRoundObserver for ProgressToolRoundObserver {
    async fn execution_started(
        &mut self,
        turn: usize,
        all_calls: &[crate::application::tool::agent::ToolCall],
        executable: &[crate::application::tool::agent::ToolCall],
    ) {
        crate::application::loop_engine::llm_log::log_tool_calls(all_calls, &self.role_name);
        if let Some(sink) = self.progress_sink.as_ref() {
            sink.emit(build_tool_calls_progress_event(
                self.source_context.clone(),
                turn,
                executable,
            ));
        }
    }

    async fn execution_finished(
        &mut self,
        execution: &RunExecutionState,
        turn: usize,
        results: &[crate::application::tool::agent::ToolExecution],
    ) {
        (self.progress)(
            Some(turn),
            &format!(
                "Tools done ({}s elapsed), {} results",
                execution.elapsed().as_secs(),
                results.len(),
            ),
        );
        for result in results {
            let output = &result.outcome.text;
            let label = if result.outcome.is_error { "ERR" } else { "OK" };
            let output = if output.len() > 300 {
                format!("{}...[{} chars]", slice_head(output, 300), output.len())
            } else {
                output.clone()
            };
            (self.progress)(
                Some(turn),
                &format!("  ← {}[{}]: {}", result.tool_name, label, output),
            );
        }
    }
}

pub(super) struct DerivedStuckObserver {
    pub progress: ProgressReporter,
}

#[async_trait]
impl crate::application::loop_engine::StuckHandlingPort for DerivedStuckObserver {
    async fn on_stuck(
        &mut self,
        execution: &RunExecutionState,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        (self.progress)(
            Some(execution.turn_count()),
            &format!("StuckGuard: {decision:?}"),
        );
        Ok(())
    }
}

pub(super) struct SubRunFinalizer {
    pub role_name: String,
    pub model_name: String,
    pub runtime_context: RuntimeContext,
    pub workspace_root: std::path::PathBuf,
    pub session_id: String,
    pub prompt: String,
    pub system: String,
    pub model_spec: Option<String>,
    pub progress_sink: Option<Arc<dyn tools::ProgressSink>>,
    pub source_context: AgentProgressSourceContext,
}

impl SubRunFinalizer {
    pub(super) async fn finish(
        self,
        execution: &mut RunExecutionState,
        launch_result: crate::application::run::launcher::RunLaunchResult,
    ) -> AgentRunTerminal {
        crate::application::loop_engine::run_finalization::RunFinalizationCoordinator::new(
            Some(self.role_name),
            self.model_name,
            super::finalize::SubRunFinalizationObserver {
                hook_port: self.runtime_context.hooks(),
                workspace_root: &self.workspace_root,
                session_id: &self.session_id,
                prompt: &self.prompt,
                system: &self.system,
                model_spec: self.model_spec.as_deref(),
                progress_sink: self.progress_sink.as_ref(),
                source_context: self.source_context,
            },
        )
        .finalize(execution, launch_result)
        .await
    }
}

#[cfg(test)]
mod tests {
    use crate::application::loop_engine::event_strategy::terminal_from_domain_event;
    use crate::domain::agent_run::{RunDomainEvent, RunId};

    #[test]
    fn terminal_domain_events_project_to_all_agent_terminal_variants() {
        let run_id = RunId::new_v7();
        let parent_run_id = Some(RunId::new_v7());
        let cases = [
            (
                RunDomainEvent::Completed {
                    run_id: run_id.clone(),
                    parent_run_id: parent_run_id.clone(),
                    result: "done".to_string(),
                    user_cancelled_step: false,
                },
                Some(tools::AgentRunTerminal::Completed {
                    result: "done".to_string(),
                }),
            ),
            (
                RunDomainEvent::Failed {
                    run_id: run_id.clone(),
                    parent_run_id: parent_run_id.clone(),
                    error: "boom".to_string(),
                },
                Some(tools::AgentRunTerminal::Failed {
                    error: "boom".to_string(),
                }),
            ),
            (
                RunDomainEvent::Cancelled {
                    run_id,
                    parent_run_id,
                },
                Some(tools::AgentRunTerminal::Cancelled),
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(terminal_from_domain_event(&event), expected);
        }
    }

    #[test]
    fn nonterminal_domain_event_does_not_create_agent_terminal() {
        let event = RunDomainEvent::Started {
            run_id: RunId::new_v7(),
            parent_run_id: Some(RunId::new_v7()),
        };
        assert_eq!(terminal_from_domain_event(&event), None);
    }
}
