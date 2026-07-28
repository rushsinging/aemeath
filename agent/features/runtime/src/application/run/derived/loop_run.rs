use super::progress::build_tool_calls_progress_event;
use crate::application::loop_engine::chat::InvocationResponse;
use crate::application::loop_engine::event_strategy::{ProgressTerminalObserver, RunEventObserver};
use crate::application::loop_engine::{LoopEngineError, ModelStep, ToolGuardDecision};
use crate::application::run::context::RuntimeContext;
use crate::application::tool::agent::Agent;
use crate::domain::agent_run::RunDomainEvent;
use crate::ports::StopReason;
use async_trait::async_trait;
use share::message::Message;
use share::string_idx::slice_head;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::AgentRunTerminal;
use tools::{AgentProgressEvent, AgentProgressKind};

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
        token: tokio_util::sync::CancellationToken,
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

pub(super) async fn launch_sub_run(
    run: crate::domain::agent_run::Run,
    mut execution: crate::application::run::execution_state::RunExecutionState,
    mut capabilities: DerivedLoopCapabilityAdapter<'_>,
) -> AgentRunTerminal {
    let _signal_propagation = CancellationPropagationGuard::new(
        capabilities.agent.ctx.cancellation(),
        capabilities.runtime_cancellation.clone(),
    );
    let _binding = match tools::ToolExecutionContextBindingGuard::bind(
        capabilities.runtime_context.tool_context_binding(),
        capabilities.agent.ctx.clone(),
    ) {
        Ok(binding) => binding,
        Err(error) => return AgentRunTerminal::Failed { error },
    };
    let active_run = capabilities.active_run.clone();
    let cancel = capabilities.runtime_cancellation.clone();
    let context = capabilities.runtime_context.clone();
    let launch_result = crate::application::run::launcher::launch_prepared(
        run,
        &mut execution,
        &context,
        cancel,
        active_run,
        &mut capabilities,
    )
    .await;
    capabilities.finish(&mut execution, launch_result).await
}

#[allow(clippy::type_complexity)]
pub(super) struct DerivedLoopCapabilityAdapter<'a> {
    pub prompt: &'a str,
    pub system: String,
    pub progress_sink: Option<Arc<dyn tools::ProgressSink>>,
    /// #1385 Task 12: Provider binding, hook, policy, tool_context_binding, config,
    /// context (via context()), usage (via usage()), and event_sink all come from
    /// `runtime_context` via accessors — no separate fields needed.
    /// Owned (not Arc) — derived context is already owned by the caller.
    pub runtime_context: RuntimeContext,
    pub max_tokens: u32,
    /// #1248 Task 7: reasoning level comes from RuntimeContext's
    /// ReasoningPort (not a duplicate static field).
    pub workspace_root: std::path::PathBuf,
    pub tool_schemas: Vec<serde_json::Value>,
    pub config_snapshot: share::config::domain::snapshot::ConfigSnapshot,
    pub language: String,
    pub agent: Agent,
    pub runtime_cancellation: tokio_util::sync::CancellationToken,
    /// #1385 Task 12: last_total_tokens eliminated — usage tracker is the single source.
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    pub session_id: String,
    pub role_name_for_log: String,
    pub model_name_for_log: String,
    pub resolved_spec: Option<String>,
    pub progress: Box<dyn Fn(Option<usize>, &str) + Send + Sync + 'a>,
    pub ctx_context_size: usize,
    pub tool_result_materializer:
        Arc<crate::application::tool::result_materialization::ToolResultMaterializer>,
    /// Input strategy: encapsulates the fixed-prompt drain logic with epoch
    /// tracking and tool-result continuation support (#1272, #1384).
    pub input_strategy: crate::application::loop_engine::input_strategy::FixedInputAdapter<'a>,
    /// #1248 Task 5: Whether this sub-run is operating in plan mode.
    pub plan_mode: bool,
}

impl<'a> DerivedLoopCapabilityAdapter<'a> {
    fn context_request_source(
        &self,
    ) -> crate::application::loop_engine::context_request::ContextRequestSource<'_> {
        crate::application::loop_engine::context_request::ContextRequestSource {
            runtime_context: &self.runtime_context,
            session_id: &self.session_id,
            system_prompt: &self.system,
            model_id: &self.model_name_for_log,
            language: &self.language,
            task_reminder: crate::ports::TaskReminderSnapshot::default(),
            agent_roles: self
                .config_snapshot
                .agents()
                .roles
                .iter()
                .filter(|(_, role)| role.enabled)
                .map(|(name, role)| (name.clone(), role.clone()))
                .collect(),
            config: self.runtime_context.config_ref(),
            context_size: self.ctx_context_size,
            max_output_tokens: self.max_tokens as usize,
            raw_tool_schemas: self.tool_schemas.clone(),
        }
    }

    async fn finish(
        self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        launch_result: crate::application::run::launcher::RunLaunchResult,
    ) -> AgentRunTerminal {
        crate::application::loop_engine::run_finalization::RunFinalizationCoordinator::new(
            Some(self.role_name_for_log.clone()),
            self.model_name_for_log.clone(),
            super::finalize::SubRunFinalizationObserver {
                hook_port: self.runtime_context.hooks(),
                workspace_root: &self.workspace_root,
                session_id: &self.session_id,
                prompt: self.prompt,
                system: &self.system,
                model_spec: self.resolved_spec.as_deref(),
                progress_sink: self.progress_sink.as_ref(),
            },
        )
        .finalize(execution, launch_result)
        .await
    }

    fn progress_turn_start(
        &self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        turn_number: usize,
    ) {
        let msg_tokens = execution.message_tokens();
        (self.progress)(
            Some(turn_number),
            &format!(
                "Agent turn {}, messages: {}, est_tokens: {}",
                turn_number,
                execution.messages_len(),
                msg_tokens
            ),
        );
    }

    fn progress_api_ok(&self, turn_number: usize, resp: &InvocationResponse) {
        (self.progress)(
            Some(turn_number),
            &format!(
                "API ok: in={} out={} stop={:?}",
                resp.usage.input_tokens.unwrap_or(0),
                resp.usage.output_tokens.unwrap_or(0),
                resp.stop_reason
            ),
        );
    }

    fn send_text_progress(&self, turn: usize, resp: &InvocationResponse) {
        if let Some(ref sink) = self.progress_sink {
            let text = resp.assistant_message.text_content();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let short = if trimmed.len() > 300 {
                    format!("{}...", slice_head(trimmed, 300))
                } else {
                    trimmed.to_string()
                };
                sink.emit(AgentProgressEvent {
                    sequence: turn,
                    kind: AgentProgressKind::Message { text: short },
                });
            }
        }
    }

    async fn invoke_shared_model(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        execution.advance_turn();
        let turn_number = execution.turn_count();
        logging::within(
            logging::LogContextPatch {
                turn: logging::FieldPatch::Set(turn_number),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
            crate::application::model::invocation::orchestrate_model_invocation(
                self, execution, cancel,
            ),
        )
        .await
    }
}

// ── Sub-agent tool-round observation ─────────────────────────────────────

struct ProgressToolRoundObserver<'a> {
    progress_sink: Option<Arc<dyn tools::ProgressSink>>,
    progress: &'a (dyn Fn(Option<usize>, &str) + Send + Sync),
    role_name: &'a str,
}

#[async_trait]
impl crate::application::tool::coordination::ToolRoundObserver for ProgressToolRoundObserver<'_> {
    async fn execution_started(
        &mut self,
        turn: usize,
        all_calls: &[crate::application::tool::agent::ToolCall],
        executable: &[crate::application::tool::agent::ToolCall],
    ) {
        crate::application::loop_engine::llm_log::log_tool_calls(all_calls, self.role_name);
        if let Some(ref sink) = self.progress_sink {
            sink.emit(build_tool_calls_progress_event(turn, executable));
        }
    }

    async fn execution_finished(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        turn: usize,
        results: &[crate::application::tool::agent::ToolExecution],
    ) {
        let calls = results
            .iter()
            .map(|result| crate::application::tool::agent::ToolCall {
                id: result.call_id.clone(),
                provider_id: result.provider_id.clone(),
                name: result.tool_name.clone(),
                index: 0,
                input: serde_json::Value::Null,
            })
            .collect::<Vec<_>>();
        let call_info = calls
            .iter()
            .map(|call| (call.id.clone(), (call.name.clone(), call.input.to_string())))
            .collect::<std::collections::HashMap<_, _>>();
        (self.progress)(
            Some(turn),
            &format!(
                "Tools done ({}s elapsed), {} results",
                execution.elapsed().as_secs(),
                results.len()
            ),
        );
        for result in results {
            let output = &result.outcome.text;
            let label = if result.outcome.is_error { "ERR" } else { "OK" };
            if let Some((name, input_short)) = call_info.get(&result.call_id) {
                (self.progress)(Some(turn), &format!("  → {}({})", name, input_short));
            }
            let output_short = if output.len() > 300 {
                format!("{}...[{} chars]", slice_head(output, 300), output.len())
            } else {
                output.clone()
            };
            let tool_name = call_info
                .get(&result.call_id)
                .map(|(name, _)| name.as_str())
                .unwrap_or("?");
            (self.progress)(
                Some(turn),
                &format!("  ← {}[{}]: {}", tool_name, label, output_short),
            );
            crate::application::loop_engine::llm_log::log_tool_result(
                &result.call_id,
                output,
                result.outcome.is_error,
                &call_info,
                self.role_name,
            );
        }
    }
}

// ── Model invocation lifecycle capability ──────────────────────────────

impl crate::application::model::invocation::ModelInvocationSource
    for DerivedLoopCapabilityAdapter<'_>
{
    fn runtime_context(&self) -> &RuntimeContext {
        &self.runtime_context
    }

    fn role(&self) -> &str {
        &self.role_name_for_log
    }

    fn request_log_context(&self, parent: &logging::LogContext) -> logging::LogContext {
        sub_request_log_context(
            parent,
            &self.model_name_for_log,
            &self.runtime_context.provider().model.provider,
            &self.role_name_for_log,
        )
    }

    fn context_size(
        &self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
    ) -> usize {
        self.ctx_context_size
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
impl crate::application::model::invocation::ModelInvocationObserver
    for DerivedLoopCapabilityAdapter<'_>
{
    async fn on_window(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
    ) {
        self.progress_turn_start(execution, execution.turn_count());
    }

    async fn on_retry(&mut self, attempt: u32, delay: std::time::Duration) {
        log::info!(
            target: crate::LOG_TARGET,
            "sub-agent model invocation retrying: attempt={} delay_ms={}",
            attempt,
            delay.as_millis(),
        );
    }

    async fn on_retry_cancelled(&mut self) {
        self.runtime_cancellation.cancel();
    }

    async fn on_response(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        response: &InvocationResponse,
        _elapsed_secs: f64,
    ) {
        self.progress_api_ok(execution.turn_count(), response);
        self.send_text_progress(execution.turn_count(), response);
    }

    async fn classify_terminal(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
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

#[async_trait]
impl crate::application::loop_engine::InputPort for DerivedLoopCapabilityAdapter<'_> {
    async fn drain_input(
        &mut self,
        expected_epoch: crate::application::loop_engine::DrainEpoch,
    ) -> Result<crate::application::loop_engine::DrainOutcome, LoopEngineError> {
        use crate::application::loop_engine::input_strategy::InputStrategy;
        self.input_strategy.drain_input(expected_epoch).await
    }

    fn schedule_internal_continuation(
        &mut self,
        kind: crate::application::loop_engine::InternalContinuationKind,
    ) {
        if matches!(
            kind,
            crate::application::loop_engine::InternalContinuationKind::ToolResults
        ) {
            self.input_strategy.has_tool_results_pending = true;
        }
    }

    async fn await_user_input(
        &mut self,
        expected_epoch: crate::application::loop_engine::DrainEpoch,
    ) -> Result<crate::application::loop_engine::DrainOutcome, LoopEngineError> {
        use crate::application::loop_engine::input_strategy::InputStrategy;
        self.input_strategy.await_user_input(expected_epoch).await
    }
}

#[async_trait]
impl crate::application::loop_engine::EventSinkPort for DerivedLoopCapabilityAdapter<'_> {
    async fn emit(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        let turn_count = execution.turn_count();
        let mut observer = ProgressTerminalObserver {
            progress: &*self.progress,
            terminal: execution.terminal_mut(),
            turn_count,
        };
        observer.emit(events).await
    }
}

#[async_trait]
impl crate::application::loop_engine::StepPersistencePort for DerivedLoopCapabilityAdapter<'_> {
    fn build_context_request(
        &self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Option<crate::ports::ContextRequest> {
        Some(
            crate::application::loop_engine::context_request::ContextRequestCoordinator::new(
                self.context_request_source(),
            )
            .build_request(run_id, step_id, execution.step_outcome()),
        )
    }

    async fn accept_step_input(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        let mut observer =
            crate::application::loop_engine::step_persistence::NoopAcceptedInputObserver;
        crate::application::loop_engine::step_persistence::StepPersistenceCoordinator::from_context(
            &self.runtime_context,
        )
        .accept_step_input(execution, step_id, &mut observer)
        .await
    }

    async fn persist_step_commit(
        &mut self,
        commit: &crate::application::loop_engine::StepCommit,
    ) -> Result<(), LoopEngineError> {
        crate::application::loop_engine::step_persistence::StepPersistenceCoordinator::from_context(
            &self.runtime_context,
        )
        .persist_step_commit(commit)
        .await
    }
}

#[async_trait]
impl crate::application::loop_engine::CompactionPort for DerivedLoopCapabilityAdapter<'_> {
    async fn needs_compaction(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        crate::application::loop_engine::compaction::CompactionCoordinator::from_context(
            &self.runtime_context,
        )
        .needs_compaction(execution)
        .await
    }

    async fn compact(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), LoopEngineError> {
        let mut observer = crate::application::loop_engine::compaction::NoopCompactionObserver;
        crate::application::loop_engine::compaction::CompactionCoordinator::from_context(
            &self.runtime_context,
        )
        .compact(execution, &mut observer)
        .await
    }
}

#[async_trait]
impl crate::application::loop_engine::ModelInvocationPort for DerivedLoopCapabilityAdapter<'_> {
    async fn invoke_model(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.invoke_shared_model(execution, cancel).await
    }
}

#[async_trait]
impl crate::application::hook::stop_coordination::StopHookObserver
    for DerivedLoopCapabilityAdapter<'_>
{
    fn stop_hook_execution_context(
        &self,
    ) -> Option<crate::application::hook::stop_coordination::StopHookExecutionContext> {
        Some(
            crate::application::hook::stop_coordination::StopHookExecutionContext::new(
                self.runtime_context.hooks(),
                self.workspace_root.clone(),
                self.session_id.clone(),
                self.language.clone(),
            ),
        )
    }
}

#[async_trait]
impl crate::application::loop_engine::ToolOrchestrationPort for DerivedLoopCapabilityAdapter<'_> {
    async fn execute_tools(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(crate::application::tool::agent::ToolCall, ToolGuardDecision)],
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        let turn = execution.turn_count();
        let context = crate::application::tool::coordination::ToolRoundContext {
            runtime_context: &self.runtime_context,
            agent: Agent {
                catalog: self.agent.catalog.clone(),
                execution: self.agent.execution.clone(),
                ctx: self.agent.ctx.clone(),
                max_tool_concurrency: self.agent.max_tool_concurrency,
                agent_semaphore: self.agent.agent_semaphore.clone(),
                workspace_persist: self.agent.workspace_persist.clone(),
                runtime_cancellation: self.agent.runtime_cancellation.clone(),
            },
            turn_context: crate::application::loop_engine::chat::RuntimeTurnContext::new(
                sdk::ChatId::from_legacy_or_new(&self.session_id),
                sdk::ChatTurnId::new_v7(),
            ),
            language: &self.language,
            workspace_root: self.agent.ctx.workspace_read().current_workspace_root(),
            session_id: &self.session_id,
            materializer: self.tool_result_materializer.as_ref(),
            log_patch: logging::LogContextPatch {
                turn: logging::FieldPatch::Set(turn),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
        };
        crate::application::tool::coordination::ToolRoundCoordinator::new(
            context,
            ProgressToolRoundObserver {
                progress_sink: self.progress_sink.clone(),
                progress: &*self.progress,
                role_name: &self.role_name_for_log,
            },
        )
        .execute(execution, run_id, step_id, calls, cancel)
        .await
    }
}

#[async_trait]
impl crate::application::loop_engine::StuckHandlingPort for DerivedLoopCapabilityAdapter<'_> {
    async fn on_stuck(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        (self.progress)(
            Some(execution.turn_count()),
            &format!("StuckGuard: {decision:?}"),
        );
        Ok(())
    }
}

impl crate::application::loop_engine::PlanApprovalPort for DerivedLoopCapabilityAdapter<'_> {
    fn needs_plan_approval(&self) -> bool {
        self.plan_mode
    }
}

#[async_trait]
impl crate::application::loop_engine::InteractionMailboxPort for DerivedLoopCapabilityAdapter<'_> {
    fn interaction_port(&self) -> &dyn crate::application::interaction::port::InteractionPort {
        self.runtime_context.interaction_ref().as_ref()
    }

    async fn publish_interaction(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        // #1248: Publish real progress event via parent UI event seam,
        // not just a debug string. The parent TUI picks these up.
        (self.progress)(
            Some(execution.turn_count()),
            &format!("Interaction: id={}", request.id),
        );
        Ok(())
    }

    fn set_pending_interaction_work(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        work: crate::application::loop_engine::PendingInteractionWork,
    ) {
        execution.set_pending_interaction_work(work);
    }
}

impl crate::application::interaction::coordinator::InteractionCompletionContextProvider
    for DerivedLoopCapabilityAdapter<'_>
{
    fn interaction_completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> crate::application::interaction::coordinator::InteractionCompletionContext<'_> {
        crate::application::interaction::coordinator::InteractionCompletionContext::new(
            self.agent.ctx.scope().clone(),
            self.agent.execution.as_ref(),
            self.tool_result_materializer.as_ref(),
            &self.session_id,
            std::sync::Arc::new(
                crate::application::run::context::RunCancellationScope::from_token(step_cancel),
            ),
        )
    }
}

impl crate::application::loop_engine::RunControlPort for DerivedLoopCapabilityAdapter<'_> {
    fn take_control(&self, _run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        None
    }
}

impl crate::application::loop_engine::RunLifecyclePort for DerivedLoopCapabilityAdapter<'_> {
    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        crate::application::loop_engine::run_lifecycle::RunLifecycleCoordinator::new(
            self.active_run.as_ref(),
            crate::application::loop_engine::run_lifecycle::NoopStepScopeObserver,
        )
        .claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        crate::application::loop_engine::run_lifecycle::RunLifecycleCoordinator::new(
            self.active_run.as_ref(),
            crate::application::loop_engine::run_lifecycle::NoopStepScopeObserver,
        )
        .claim_cancellation(run_id)
    }

    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        crate::application::loop_engine::run_lifecycle::RunLifecycleCoordinator::new(
            self.active_run.as_ref(),
            crate::application::loop_engine::run_lifecycle::NoopStepScopeObserver,
        )
        .register_step_scope(run_id, step_id, cancel);
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
