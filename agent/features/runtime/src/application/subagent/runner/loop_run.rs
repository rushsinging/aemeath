use super::finalize::{finalize_sub_agent, AgentRunOutcome, AgentRunStatus};
use super::loop_helpers::append_tool_results;
use super::progress::build_tool_calls_progress_event;
use crate::application::context_coordination::ContextCoordinator;
use crate::application::loop_engine::event_strategy::{EventStrategy, SubEventStrategy};
use crate::application::loop_engine::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::application::loop_engine::shared::compact_core;
use crate::application::loop_engine::tool_strategy::{self, ToolStrategy};
use crate::application::loop_engine::{
    LoopEngineError, LoopEnginePort, ModelStep, ToolGuardDecision, ToolStep,
};
use crate::application::main_loop::looping::InvocationResponse;
use crate::application::runtime_context::RuntimeContext;
use crate::application::subagent::Agent;
use crate::domain::agent_run::{RunDomainEvent, ToolCallStatus};
use crate::ports::{InvocationOptions, InvocationRequest, StopReason};
use async_trait::async_trait;
use provider::RequestSystemBlock;
use share::message::Message;
use share::string_idx::slice_head;
use std::sync::Arc;
use std::time::Instant;
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

/// #1385 Task 12: Canonical sub-agent event sink (noop by design —
/// sub-agents push events through parent).  Used by both [`SubAgentRun`]
/// (for `InvocationEventReducer`) and [`derive_sub_run`] (for the
/// derived [`RuntimeContext`]'s event_sink) so there is one definition,
/// not duplicated inline noops.
#[derive(Clone)]
pub(super) struct SubAgentEventSink;

impl crate::application::main_loop::looping::ChatEventSink for SubAgentEventSink {
    fn send_event<'a>(
        &'a self,
        _event: crate::application::main_loop::looping::RuntimeStreamEvent,
    ) -> crate::application::main_loop::looping::EventFuture<'a> {
        Box::pin(async {})
    }

    fn try_send_event(&self, _event: crate::application::main_loop::looping::RuntimeStreamEvent) {}
}

pub(super) fn messages_for_llm(messages: &[Message]) -> Vec<Message> {
    messages.iter().map(Message::to_llm_view).collect()
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

pub(super) struct SubAgentLaunch<'a> {
    pub run: crate::domain::agent_run::Run,
    pub adapter: SubAgentRun<'a>,
}

impl SubAgentLaunch<'_> {
    pub async fn run(mut self) -> AgentRunTerminal {
        let _signal_propagation = CancellationPropagationGuard::new(
            self.adapter.agent.ctx.cancellation(),
            self.adapter.runtime_cancellation.clone(),
        );
        let _binding = match tools::ToolExecutionContextBindingGuard::bind(
            self.adapter.runtime_context.tool_context_binding(),
            self.adapter.agent.ctx.clone(),
        ) {
            Ok(binding) => binding,
            Err(error) => return AgentRunTerminal::Failed { error },
        };
        let active_run = self.adapter.active_run.clone();
        let cancel = self.adapter.runtime_cancellation.clone();
        let context = self.adapter.runtime_context.clone();
        let execution = std::mem::take(&mut self.adapter.execution);
        let (launch_result, execution) = crate::application::run_launcher::launch_prepared(
            self.run,
            execution,
            &context,
            cancel,
            active_run,
            &mut self.adapter,
        )
        .await;
        self.adapter.execution = execution;
        self.adapter.finish(launch_result).await
    }
}

#[allow(clippy::type_complexity)]
pub(super) struct SubAgentRun<'a> {
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
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    /// Input strategy: encapsulates the fixed-prompt drain logic with epoch
    /// tracking and tool-result continuation support (#1272, #1384).
    pub input_strategy: crate::application::loop_engine::input_strategy::SubInputStrategy<'a>,
    /// #1248 Task 5: Whether this sub-run is operating in plan mode.
    pub plan_mode: bool,
    /// Loop 工作数据的唯一 owner；首批迁移 interaction continuation 工作集。
    pub execution: crate::application::run_execution_state::RunExecutionState,
}

impl<'a> SubAgentRun<'a> {
    /// #1385 Task 12: Construct a fresh ContextCoordinator from the runtime
    /// context's ContextPort.  No stored copy — each use builds its own.
    #[inline]
    fn ctx_coordinator(&self) -> ContextCoordinator {
        ContextCoordinator::new(self.runtime_context.context())
    }

    fn freeze_request(
        &self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> crate::ports::ContextRequest {
        let raw_tool_schemas = self.tool_schemas.clone();
        let tool_schemas = raw_tool_schemas
            .iter()
            .filter_map(|schema| {
                Some(crate::ports::ModelToolSchema {
                    name: schema.get("name")?.as_str()?.to_string(),
                    description: schema.get("description")?.as_str()?.to_string(),
                    input_schema: schema.get("input_schema")?.clone(),
                })
            })
            .collect();
        crate::ports::ContextRequest {
            session_id: crate::ports::SessionId::new(&self.session_id),
            request_id: crate::ports::ContextRequestId::new(uuid::Uuid::now_v7().to_string()),
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            pending_messages: self
                .execution
                .messages_slice_from(
                    self.execution.committed_message_count() + self.execution.accepted_input_len(),
                )
                .to_vec(),
            system_prompt: crate::ports::SystemPromptSpec::new(&self.system),
            model_id: self.model_name_for_log.clone(),
            // #1248 Task 7: effective reasoning from assembled ReasoningPort.
            effective_reasoning: *self
                .runtime_context
                .reasoning_ref()
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            task_reminder: crate::ports::TaskReminderSnapshot::default(),
            language: crate::ports::Language::new(&self.language),
            agent_roles: self
                .config_snapshot
                .agents()
                .roles
                .iter()
                .filter(|(_, role)| role.enabled)
                .map(|(name, role)| (name.clone(), role.clone()))
                .collect(),
            config_snapshot: self.config_snapshot.clone(),
            context_size: self.ctx_context_size,
            max_output_tokens: self.max_tokens as usize,
            last_api_total_tokens: self.runtime_context.usage().get(),
            tool_schemas,
            tool_schema_tokens: context::compact::estimate_tool_schemas_tokens(&raw_tool_schemas),
        }
    }

    async fn finish(
        mut self,
        launch_result: crate::application::run_launcher::RunLaunchResult,
    ) -> AgentRunTerminal {
        let loop_result = match launch_result {
            crate::application::run_launcher::RunLaunchResult::Terminal => Ok(()),
            crate::application::run_launcher::RunLaunchResult::Failed(error) => Err(error),
        };

        // A normal terminal path is recorded by `emit` from the authoritative
        // RunDomainEvent. Keep an infrastructure fallback so finalization still
        // runs if the engine itself cannot finish a transition.
        let terminal = self
            .execution
            .take_terminal()
            .unwrap_or_else(|| AgentRunTerminal::Failed {
                error: loop_result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| {
                        "shared run loop ended without a terminal event".to_string()
                    }),
            });

        let outcome = AgentRunOutcome {
            status: match &terminal {
                AgentRunTerminal::Completed { .. } => AgentRunStatus::Completed,
                AgentRunTerminal::Failed { error } => {
                    if error.starts_with("run timed out after ") {
                        AgentRunStatus::TimedOut
                    } else {
                        AgentRunStatus::Failed(error.clone())
                    }
                }
                AgentRunTerminal::Cancelled => AgentRunStatus::Cancelled,
            },
            turns: self.execution.turn_count(),
            duration: self.execution.elapsed(),
            role: Some(self.role_name_for_log.clone()),
            model: self.model_name_for_log.clone(),
        };
        let output = terminal.output();
        finalize_sub_agent(
            &outcome,
            &self.runtime_context.hooks(),
            &self.workspace_root,
            &self.session_id,
            self.prompt,
            &self.system,
            self.resolved_spec.as_deref(),
            &output,
            self.progress_sink.as_ref(),
        )
        .await;

        terminal
    }

    fn progress_turn_start(&self, turn_number: usize) {
        let msg_tokens = self.execution.message_tokens();
        (self.progress)(
            Some(turn_number),
            &format!(
                "Agent turn {}, messages: {}, est_tokens: {}",
                turn_number,
                self.execution.messages_len(),
                msg_tokens
            ),
        );
    }

    fn log_input(&self, system_blocks: &[RequestSystemBlock], tool_schemas: &[serde_json::Value]) {
        log_llm_input(
            self.execution.messages(),
            self.execution.committed_message_count(),
            system_blocks,
            tool_schemas,
            &self.role_name_for_log,
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

    fn log_output(&self, resp: &InvocationResponse, api_elapsed: f64) {
        log_llm_output_and_tool_calls(
            &self.runtime_context.provider().model.provider,
            resp,
            &[],
            api_elapsed,
            &self.role_name_for_log,
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

    fn log_tool_calls(&self, tool_calls: &[crate::application::subagent::ToolCall]) {
        crate::application::loop_engine::llm_log::log_tool_calls(
            tool_calls,
            &self.role_name_for_log,
        );
    }

    fn build_call_info(
        &self,
        tool_calls: &[crate::application::subagent::ToolCall],
    ) -> std::collections::HashMap<sdk::ids::ToolCallId, (String, String)> {
        tool_calls
            .iter()
            .map(|call| {
                let input_summary = call.input.to_string();
                let input_short = if input_summary.len() > 200 {
                    format!("{}...", slice_head(&input_summary, 200))
                } else {
                    input_summary
                };
                (call.id.clone(), (call.name.clone(), input_short))
            })
            .collect()
    }

    async fn invoke_model_impl(
        &mut self,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.execution.advance_turn();
        let turn_number = self.execution.turn_count();
        logging::within(
            logging::LogContextPatch {
                turn: logging::FieldPatch::Set(turn_number),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
            async move {
                self.progress_turn_start(turn_number);
                let window = if let Some(window) = self.execution.context_window().cloned() {
                    Some(window)
                } else if let Some(request) = self.execution.context_request() {
                    let coordinator = self.ctx_coordinator();                    Some(
                        coordinator
                            .build_window(request)
                            .await
                            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?,
                    )
                } else {
                    None
                };
                let messages_for_api = window
                    .as_ref()
                    .map(|window| messages_for_llm(&window.messages))
                    .unwrap_or_else(|| messages_for_llm(self.execution.messages()));
                let (effective_blocks, raw_tool_schemas) = match &window {
                    Some(w) => {
                        let ctx = crate::application::loop_engine::llm_strategy::extract_invocation_context(w);
                        let tools = ctx.tool_schemas;
                        (ctx.system_blocks, tools)
                    }
                    None => (Vec::new(), Vec::new()),
                };
                let effective_tools = window
                    .as_ref()
                    .map(|window| window.tool_schemas.clone())
                    .unwrap_or_default();
                self.log_input(&effective_blocks, &raw_tool_schemas);
                *self.execution.context_window_mut() = window;
                let mut coordinator =                    crate::application::model_invocation::ModelInvocationCoordinator::new();
                let api_start = Instant::now();
                let resp = loop {
                    let request_context = sub_request_log_context(
                        &logging::capture(),
                        &self.model_name_for_log,
                        &self.runtime_context.provider().model.provider,
                        &self.role_name_for_log,
                    );
                    let response = logging::instrument(request_context, async {
                            let mut reducer =
                                crate::application::main_loop::looping::InvocationEventReducer::new(
                                    SubAgentEventSink,
                                );
                            let provider = self.runtime_context.provider().provider.clone();
                            let model = self.runtime_context.provider().model.clone();
                            let max_tokens = self.max_tokens;
                            let level = {
                                use crate::application::loop_engine::llm_strategy::LlmStrategy;
                                self.reasoning_level()
                            };
                            let committed_delta = {
                                use crate::application::loop_engine::llm_strategy::LlmStrategy;
                                self.committed_delta()
                            };
                            let system = effective_blocks.clone();
                            let messages = messages_for_api.clone();
                            let tools = effective_tools.clone();
                            let cancellation = self.runtime_cancellation.clone();
                            let invocation_fut = async {
                                let mut request = InvocationRequest::new(
                                    model,
                                    messages,
                                    InvocationOptions::new(max_tokens, level),
                                );
                                request.system = system;
                                request.tools = tools;
                                request.cancellation = cancellation.clone();
                                match provider.invoke(request, &cancellation).await {
                                    Ok(stream) => {
                                        coordinator
                                            .pull_stream(stream, &cancellation, committed_delta, |event| {
                                            reducer.apply(event)
                                        })
                                        .await
                                }
                                Err(error) => Err((error, false)),
                            }
                        };
                        invocation_fut.await
                    })
                    .await;

                    match response {
                        Ok((resp, _)) => break resp,
                        Err((error, _))
                            if error.is_cancelled()
                                || self.agent.ctx.cancellation().is_cancelled() =>
                        {
                            self.runtime_cancellation.cancel();
                            return Err(LoopEngineError::Cancelled);
                        }
                        Err((error, visible_delta)) => {
                                let step = coordinator
                                    .handle_failure(&error, visible_delta, &self.runtime_cancellation)
                                    .await;
                                crate::application::loop_engine::llm_strategy::map_retry_outcome(
                                    step,
                                    &error.to_string(),
                                    self,
                                )
                                .await?;
                            }
                    }
                };

                // #1385 Task 12: Write token usage to RuntimeContext's tracker — single source.
                let total_tokens =
                    crate::application::token_usage::normalized_total_tokens(&resp.usage);
                self.runtime_context.usage().update(total_tokens);
                self.progress_api_ok(turn_number, &resp);

                let est = self
                    .execution
                    .context_window()                    .map(|window| &window.token_estimation);
                let usage = crate::application::loop_engine::llm_strategy::build_step_token_usage(
                    &resp,
                    self.ctx_context_size as u64,
                    est.map_or(0, |e| e.system_tokens),
                    est.map_or(0, |e| e.tool_schema_tokens),
                    est.map_or(0, |e| e.message_tokens),
                );

                self.execution.append_message(resp.assistant_message.clone());
                self.log_output(&resp, api_start.elapsed().as_secs_f64());
                self.send_text_progress(turn_number, &resp);

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                if resp.stop_reason == StopReason::MaxOutputTokens {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "turn {}: 模型响应触发 max_tokens 限制，注入分块提示",
                        turn_number,
                    );
                    self.execution.append_message(Message::user(
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
                                text: resp.assistant_message.text_content(),
                                calls: Vec::new(),
                            },
                            usage,
                        ));
                    }
                }

                if should_complete_after_model_response(tool_calls.is_empty()) {
                    let text = resp.assistant_message.text_content();
                    return Ok((ModelStep::Complete { text }, usage));
                }

                Ok((
                    ModelStep::Tools {
                        text: resp.assistant_message.text_content(),
                        calls: tool_calls,
                    },
                    usage,
                ))
            },
        )
        .await
    }

    async fn execute_tools_impl(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(crate::application::subagent::ToolCall, ToolGuardDecision)],
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        let turn_number = self.execution.turn_count();
        logging::within(
            logging::LogContextPatch {
                turn: logging::FieldPatch::Set(turn_number),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
            async move {
                if calls.is_empty() {
                    return Ok(ToolStep::Continue);
                }
                let prepared = crate::application::tool_coordination::prepare_tool_round(
                    calls,
                    &self.agent.catalog,
                    self.runtime_context.policy().as_ref(),
                    run_id,
                    step_id,
                    &self.agent.ctx.workspace_read().current_workspace_root(),
                );
                let allowed_calls = prepared
                    .executable
                    .iter()
                    .map(|prepared| prepared.call.clone())
                    .collect::<Vec<_>>();
                let fuse_bypassed = prepared.fuse_bypassed;
                let executable = prepared.executable;
                let mut results = prepared.guard_blocked;
                results.extend(
                    prepared
                        .denied
                        .into_iter()
                        .map(crate::application::tool_coordination::denied_tool_execution),
                );
                let all_calls: Vec<_> = calls.iter().map(|(call, _)| call.clone()).collect();
                self.log_tool_calls(&all_calls);
                let call_info = self.build_call_info(&all_calls);
                if let Some(ref sink) = self.progress_sink {
                    sink.emit(build_tool_calls_progress_event(turn_number, &allowed_calls));
                }

                let cancellation = self.agent.ctx.cancellation();
                let mut executed = tokio::select! {
                    _ = cancellation.cancelled() => {
                        return Err(LoopEngineError::Cancelled);
                    }
                    executed = self.agent.execute_prepared_tools(&executable) => executed,
                };
                results.append(&mut executed);
                let results = crate::application::tool_coordination::restore_tool_call_order(
                    &all_calls, results,
                );
                self.progress_tools_done(turn_number, results.len());
                self.log_result_summaries(turn_number, &results, &call_info);
                self.log_tool_results(turn_number, &results, &call_info);
                append_tool_results(
                    self.tool_result_materializer.as_ref(),
                    self.execution.messages_mut(),
                    results,
                    &self.session_id,
                )
                .await;
                // #1384: Mark that tool results are pending so drain_input
                // returns InternalContinuation instead of EmptyAndSealed.
                self.mark_tool_results_pending();
                Ok(tool_strategy::step_from_fuse_bypass(fuse_bypassed))
            },
        )
        .await
    }
}

fn should_complete_after_model_response(has_no_tool_calls: bool) -> bool {
    has_no_tool_calls
}

// ── ToolStrategy impl ─────────────────────────────────────────────────

impl ToolStrategy for SubAgentRun<'_> {
    fn mark_tool_results_pending(&mut self) {
        self.input_strategy.has_tool_results_pending = true;
    }
}

// ── LlmStrategy impl ─────────────────────────────────────────────────

#[async_trait]
impl crate::application::loop_engine::llm_strategy::LlmStrategy for SubAgentRun<'_> {
    /// #1248 Task 7: reasoning level from the assembled RuntimeContext's
    /// ReasoningPort (not a static `self.level` field). This ensures
    /// both Main and Sub read via the same port accessor.
    fn reasoning_level(&self) -> crate::ports::ReasoningLevel {
        *self
            .runtime_context
            .reasoning_ref()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn committed_delta(&self) -> bool {
        false
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
}

#[async_trait]
impl crate::application::loop_engine::InputPort for SubAgentRun<'_> {
    async fn drain_input(
        &mut self,
        expected_epoch: crate::application::loop_engine::DrainEpoch,
    ) -> Result<crate::application::loop_engine::DrainOutcome, LoopEngineError> {
        use crate::application::loop_engine::input_strategy::InputStrategy;
        self.input_strategy.drain_input(expected_epoch).await
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
impl crate::application::loop_engine::EventSinkPort for SubAgentRun<'_> {
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        let turn_count = self.execution.turn_count();
        let mut strategy = SubEventStrategy {
            progress: &*self.progress,
            terminal: self.execution.terminal_mut(),
            turn_count,
        };
        strategy.emit(events).await
    }
}

#[async_trait]
impl crate::application::loop_engine::StepPersistencePort for SubAgentRun<'_> {
    fn freeze_step(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        _inputs: &[crate::application::loop_engine::LoopInput],
    ) {
        self.execution
            .replace_accepted_input(if self.execution.committed_message_count() == 0 {
                self.execution
                    .messages()
                    .first()
                    .filter(|message| message.role == share::message::Role::User)
                    .cloned()
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            });
        self.execution
            .replace_context_projection(self.freeze_request(run_id, step_id), None);
    }

    async fn accept_step_input(&mut self, step_id: &sdk::RunStepId) -> Result<(), LoopEngineError> {
        let request = self
            .execution
            .context_request()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        debug_assert_eq!(&request.step_id, step_id);
        if self.execution.accepted_input().is_empty() {
            return Ok(());
        }
        self.ctx_coordinator()
            .append_accepted_input(request, self.execution.accepted_input().to_vec())
            .await
            .map(|_| ())
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))
    }

    async fn finalize_step(&mut self, step_id: &sdk::RunStepId) -> Result<(), LoopEngineError> {
        let (Some(request), Some(window)) = (
            self.execution.context_request(),
            self.execution.context_window(),
        ) else {
            return Ok(());
        };
        debug_assert_eq!(&request.step_id, step_id);
        let messages = self
            .execution
            .messages_slice_from(
                self.execution.committed_message_count() + self.execution.accepted_input_len(),
            )
            .to_vec();
        let coordinator = self.ctx_coordinator();
        coordinator
            .append_finalized(
                request,
                step_id.clone(),
                window.backing_revision,
                crate::ports::FinalizeCause::Completed,
                messages,
                vec![],
                self.runtime_context.usage().get(),
            )
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.execution.commit_all_messages();
        Ok(())
    }

    async fn finalize_cancelled_step(
        &mut self,
        step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        let (Some(request), Some(window)) = (
            self.execution.context_request(),
            self.execution.context_window(),
        ) else {
            return Ok(());
        };
        debug_assert_eq!(&request.step_id, step_id);
        let messages = self
            .execution
            .messages_slice_from(
                self.execution.committed_message_count() + self.execution.accepted_input_len(),
            )
            .to_vec();
        let coordinator = self.ctx_coordinator();
        coordinator
            .append_finalized(
                request,
                step_id.clone(),
                window.backing_revision,
                crate::ports::FinalizeCause::UserCancelledStep,
                messages,
                vec![],
                self.runtime_context.usage().get(),
            )
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.execution.commit_all_messages();
        Ok(())
    }
}

#[async_trait]
impl crate::application::loop_engine::CompactionPort for SubAgentRun<'_> {
    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        let coordinator = self.ctx_coordinator();
        let (needed, window) =
            crate::application::loop_engine::shared::needs_compaction_with_window(
                self.execution.context_request(),
                &coordinator,
            )
            .await?;
        *self.execution.context_window_mut() = Some(window);
        Ok(needed)
    }

    async fn compact(
        &mut self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), LoopEngineError> {
        let source_revision = self
            .execution
            .context_window()
            .map(|window| window.backing_revision)
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        let coordinator = self.ctx_coordinator();
        let request = self.execution.context_request().cloned();
        compact_core(
            request.as_ref(),
            source_revision,
            &coordinator,
            &self.runtime_context.usage(),
            self.execution.context_window_mut(),
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl crate::application::loop_engine::ModelInvocationPort for SubAgentRun<'_> {
    async fn invoke_model(
        &mut self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.invoke_model_impl().await
    }
}

#[async_trait]
impl crate::application::loop_engine::StopHookPort for SubAgentRun<'_> {
    /// #1248 Task 6: Shared Loop evaluates Stop hook via this seam.
    /// Sub agents use the same `evaluate_stop_hook` coordinator as Main,    /// but with their RuntimeContext's hook port. Feedback is injected
    /// through the same message stream for blocked retries.
    async fn evaluate_stop_hook(
        &mut self,
        turns: usize,
    ) -> Result<crate::application::stop_hook_coordination::StopHookDecision, LoopEngineError> {
        let decision = crate::application::stop_hook_coordination::evaluate_stop_hook(
            &self.runtime_context.hooks(),
            turns,
            &self.workspace_root,
        )
        .await;

        if let crate::application::stop_hook_coordination::StopHookDecision::Block(ref block) =
            &decision
        {
            let feedback =
                crate::application::stop_hook_coordination::materialize_stop_hook_feedback(
                    &block.detail,
                    &block.reason,
                    &self.session_id,
                    &self.language,
                )
                .await;
            let llm_text = format!(
                "<system-reminder>\n{}\n</system-reminder>",
                feedback.llm_text
            );
            let msg = share::message::Message::stop_hook_feedback(llm_text, feedback.payload);
            self.execution.append_message(msg);
            // Sub-event sink is a noop, but we still push the message.
        }

        Ok(decision)
    }
}

#[async_trait]
impl crate::application::loop_engine::ToolOrchestrationPort for SubAgentRun<'_> {
    async fn execute_tools(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(crate::application::subagent::ToolCall, ToolGuardDecision)],
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        self.execute_tools_impl(run_id, step_id, calls, cancel)
            .await
    }
}

#[async_trait]
impl crate::application::loop_engine::StuckHandlingPort for SubAgentRun<'_> {
    async fn on_stuck(
        &mut self,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        (self.progress)(
            Some(self.execution.turn_count()),
            &format!("StuckGuard: {decision:?}"),
        );
        Ok(())
    }
}

impl crate::application::loop_engine::PlanApprovalPort for SubAgentRun<'_> {
    fn needs_plan_approval(&self) -> bool {
        self.plan_mode
    }
}

impl crate::application::loop_engine::ExecutionStatePort for SubAgentRun<'_> {
    fn execution_state_mut(
        &mut self,
    ) -> &mut crate::application::run_execution_state::RunExecutionState {
        &mut self.execution
    }
}

#[async_trait]
impl LoopEnginePort for SubAgentRun<'_> {}

#[async_trait]
impl crate::application::loop_engine::InteractionMailboxPort for SubAgentRun<'_> {
    fn interaction_port(&self) -> &dyn crate::application::interaction::InteractionPort {
        self.runtime_context.interaction_ref().as_ref()
    }

    async fn publish_interaction(
        &mut self,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        // #1248: Publish real progress event via parent UI event seam,
        // not just a debug string. The parent TUI picks these up.
        (self.progress)(
            Some(self.execution.turn_count()),
            &format!("Interaction: id={}", request.id),
        );
        Ok(())
    }

    fn store_interaction(
        &mut self,
        metadata: crate::application::interaction::InteractionRequestMetadata,
        receiver: tokio::sync::oneshot::Receiver<
            crate::application::interaction::InteractionCompletion,
        >,
    ) -> Result<(), LoopEngineError> {
        self.execution
            .store_interaction_receiver(metadata, receiver);
        Ok(())
    }

    async fn poll_interaction(
        &mut self,
    ) -> Result<Option<crate::application::interaction::InteractionResolution>, LoopEngineError>
    {
        let mut resolved = None;
        let mut remaining = Vec::new();
        for (metadata, mut rx) in self.execution.take_interaction_receivers() {
            match rx.try_recv() {
                Ok(completion) => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[SubAgentRun::poll_interaction] resolved rid={:?}",
                        metadata.request_id
                    );
                    resolved = Some(
                        crate::application::interaction::InteractionResolution::Resolved {
                            metadata,
                            completion,
                        },
                    );
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[SubAgentRun::poll_interaction] closed rid={:?}",
                        metadata.request_id
                    );
                    resolved = Some(
                        crate::application::interaction::InteractionResolution::Closed { metadata },
                    );
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    remaining.push((metadata, rx)); // still pending
                }
            }
        }
        self.execution.replace_interaction_receivers(remaining);
        Ok(resolved)
    }

    fn set_pending_interaction_work(
        &mut self,
        work: crate::application::loop_engine::PendingInteractionWork,
    ) {
        self.execution.set_pending_interaction_work(work);
    }

    async fn finish_interaction_work(
        &mut self,
        metadata: &crate::application::interaction::InteractionRequestMetadata,
        completion: &crate::application::interaction::InteractionCompletion,
        _cancel: &CancellationToken,
    ) -> Result<crate::application::loop_engine::InteractionWorkOutcome, LoopEngineError> {
        use crate::application::interaction::InteractionCompletion;
        use crate::application::subagent::ToolExecution;
        use crate::domain::agent_run::InteractionContinuation;

        // Take the current work: current item resolves, queue stays for next.
        let work = self.execution.take_pending_interaction_work();
        let current = work.as_ref().and_then(|w| w.current.clone());
        let remaining_queue: Vec<_> = work.as_ref().map(|w| w.queue.clone()).unwrap_or_default();

        let (call_id, status) = match (&metadata.continuation, completion) {
            (
                InteractionContinuation::CompleteToolCall(id),
                InteractionCompletion::Replied(reply),
            ) => {
                // Use the original call's provider_id/name from the suspended_call.
                let (provider_id, tool_name) = current
                    .as_ref()
                    .and_then(|ci| ci.suspended_call.as_ref())
                    .map(|sc| (sc.call.provider_id.clone(), sc.call.name.clone()))
                    .unwrap_or_else(|| (String::new(), "AskUserQuestion".to_string()));
                if let sdk::InteractionReply::UserQuestions(answers) = reply {
                    let text = answers
                        .iter()
                        .enumerate()
                        .map(|(i, a)| format!("Q{}: {}", i + 1, a.0))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let outcome = tools::ToolOutcome {
                        text,
                        data: serde_json::json!({"status": "ok", "answers": answers}),
                        is_error: false,
                        images: Vec::new(),
                    };
                    let execution =
                        ToolExecution::from_parts(id.clone(), provider_id, tool_name, outcome);
                    append_tool_results(
                        self.tool_result_materializer.as_ref(),
                        self.execution.messages_mut(),
                        vec![execution],
                        &self.session_id,
                    )
                    .await;
                    self.mark_tool_results_pending();
                    (id.clone(), ToolCallStatus::Success)
                } else {
                    (id.clone(), ToolCallStatus::Error)
                }
            }
            (
                InteractionContinuation::CompleteToolCall(id),
                InteractionCompletion::Cancelled(_),
            ) => {
                let (provider_id, tool_name) = current
                    .as_ref()
                    .and_then(|ci| ci.suspended_call.as_ref())
                    .map(|sc| (sc.call.provider_id.clone(), sc.call.name.clone()))
                    .unwrap_or_else(|| (String::new(), "AskUserQuestion".to_string()));
                let outcome = tools::ToolOutcome::error("user cancelled interaction");
                let execution =
                    ToolExecution::from_parts(id.clone(), provider_id, tool_name, outcome);
                append_tool_results(
                    self.tool_result_materializer.as_ref(),
                    self.execution.messages_mut(),
                    vec![execution],
                    &self.session_id,
                )
                .await;
                (id.clone(), ToolCallStatus::Cancelled)
            }
            (
                InteractionContinuation::ContinueToolApproval(id),
                InteractionCompletion::Replied(reply),
            ) => {
                if matches!(
                    reply,
                    sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve)
                ) {
                    // Execute via agent's tool_execution port using the stored
                    // ApprovalRequiredCall from current — no re-policy.
                    if let Some(approval_call) =
                        current.as_ref().and_then(|ci| ci.approval_call.clone())
                    {
                        let call = &approval_call.call;
                        let mut input = call.input.clone();
                        tools::strip_runtime_meta(&mut input);
                        let invocation = tools::ToolInvocation::new(
                            call.name.as_str(),
                            input,
                            self.agent.ctx.scope().clone(),
                        )
                        .with_authorization(approval_call.authorization);
                        let domain = self
                            .agent
                            .execution
                            .execute(invocation, self.agent.ctx.cancellation().as_ref())
                            .await;
                        let outcome = crate::application::subagent::legacy_outcome(domain);
                        let execution = ToolExecution::from_parts(
                            id.clone(),
                            call.provider_id.clone(),
                            call.name.clone(),
                            outcome.clone(),
                        );
                        append_tool_results(
                            self.tool_result_materializer.as_ref(),
                            self.execution.messages_mut(),
                            vec![execution],
                            &self.session_id,
                        )
                        .await;
                        self.mark_tool_results_pending();
                        let status = if outcome.is_error {
                            ToolCallStatus::Error
                        } else {
                            ToolCallStatus::Success
                        };
                        (id.clone(), status)
                    } else {
                        // No approval_call stored — error
                        let outcome =
                            tools::ToolOutcome::error("tool approval call data not found");
                        let execution = ToolExecution::from_parts(
                            id.clone(),
                            String::new(),
                            "ToolApproval".to_string(),
                            outcome,
                        );
                        append_tool_results(
                            self.tool_result_materializer.as_ref(),
                            self.execution.messages_mut(),
                            vec![execution],
                            &self.session_id,
                        )
                        .await;
                        (id.clone(), ToolCallStatus::Error)
                    }
                } else {
                    // Denied — no execution
                    (id.clone(), ToolCallStatus::Cancelled)
                }
            }
            (
                InteractionContinuation::ContinueToolApproval(id),
                InteractionCompletion::Cancelled(_),
            ) => (id.clone(), ToolCallStatus::Cancelled),
            _ => {
                return Ok(
                    crate::application::loop_engine::InteractionWorkOutcome::Completed {
                        call_id: sdk::ToolCallId::new_v7(),
                        status: ToolCallStatus::Success,
                        remaining_queue,
                    },
                );
            }
        };

        Ok(
            crate::application::loop_engine::InteractionWorkOutcome::Completed {
                call_id,
                status,
                remaining_queue,
            },
        )
    }
}

impl crate::application::loop_engine::RunControlPort for SubAgentRun<'_> {
    fn take_control(&self, _run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        None
    }
}

impl crate::application::loop_engine::RunLifecyclePort for SubAgentRun<'_> {
    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_cancellation(run_id)
    }

    fn register_step_scope(
        &self,
        _run_id: &sdk::RunId,
        _step_id: sdk::RunStepId,
        _cancel: CancellationToken,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::messages_for_llm;
    use crate::application::loop_engine::event_strategy::terminal_from_domain_event;
    use crate::domain::agent_run::{RunDomainEvent, RunId};
    use share::message::{ContentBlock, Message, Role};

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

    #[test]
    fn model_response_with_tool_calls_is_not_completed_by_end_turn() {
        assert!(!super::should_complete_after_model_response(false));
        assert!(super::should_complete_after_model_response(true));
    }

    #[test]
    fn messages_for_llm_converts_structured_tool_result_to_text() {
        let messages = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: serde_json::json!({"stdout": "structured output"}),
                is_error: false,
                text: Some("plain output".to_string()),
            }],
            metadata: None,
        }];

        let api_messages = messages_for_llm(&messages);

        let ContentBlock::ToolResult { content, text, .. } = &api_messages[0].content[0] else {
            panic!("expected tool result");
        };
        assert_eq!(content, "plain output");
        assert!(text.is_none());
        let ContentBlock::ToolResult {
            content: original_content,
            text: original_text,
            ..
        } = &messages[0].content[0]
        else {
            panic!("expected original tool result");
        };
        assert_eq!(
            original_content,
            &serde_json::json!({"stdout": "structured output"})
        );
        assert_eq!(original_text.as_deref(), Some("plain output"));
    }
}
