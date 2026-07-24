use super::finalize::{finalize_sub_agent, AgentRunOutcome, AgentRunStatus};
use super::loop_helpers::append_tool_results;
use super::progress::build_tool_calls_progress_event;
use crate::application::context_coordination::ContextCoordinator;
use crate::application::loop_engine::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::application::loop_engine::{
    LoopEngineError, ModelStep, RunLoopPort, ToolGuardDecision, ToolStep,
};
use crate::application::main_loop::looping::InvocationResponse;
use crate::application::subagent::Agent;
use crate::domain::agent_run::{RunDomainEvent, RunSpec};
use crate::ports::{
    InvocationOptions, InvocationRequest, ProviderBinding, ReasoningLevel, StopReason,
};
use async_trait::async_trait;
use provider::RequestSystemBlock;
use share::message::Message;
use share::string_idx::slice_head;
use std::sync::Arc;
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

#[derive(Clone)]
struct SubAgentEventSink;

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

#[allow(clippy::type_complexity)]
pub(super) struct SubAgentRun<'a> {
    pub prompt: &'a str,
    pub system: String,
    pub progress_sink: Option<Arc<dyn tools::ProgressSink>>,
    pub binding: Arc<ProviderBinding>,
    pub max_tokens: u32,
    pub level: ReasoningLevel,
    pub hook_port: Arc<dyn hook::HookPort>,
    pub workspace_root: std::path::PathBuf,
    pub tool_schemas: Vec<serde_json::Value>,
    pub config_snapshot: share::config::domain::snapshot::ConfigSnapshot,
    pub language: String,
    pub messages: Vec<Message>,
    pub committed_message_count: usize,
    pub context: ContextCoordinator,
    pub context_request: Option<crate::ports::ContextRequest>,
    pub accepted_input: Vec<Message>,
    pub context_window: Option<crate::ports::ContextWindow>,
    pub log_request_messages: Box<dyn Fn(usize, &[Message]) + Send + Sync + 'a>,
    pub agent: Agent,
    pub runtime_cancellation: tokio_util::sync::CancellationToken,
    pub timeout: std::time::Duration,
    pub turn_count: usize,
    pub last_total_tokens: Option<u64>,
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    pub terminal: Option<AgentRunTerminal>,
    pub start_time: std::time::Instant,
    pub session_id: String,
    pub run_id: sdk::RunId,
    pub parent_run_id: Option<sdk::RunId>,
    pub role_name_for_log: String,
    pub model_name_for_log: String,
    pub resolved_spec: Option<String>,
    pub progress: Box<dyn Fn(Option<usize>, &str) + Send + Sync + 'a>,
    pub ctx_context_size: usize,
    pub tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    pub policy: Arc<dyn policy::PolicyPort>,
    pub tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    /// Input strategy: encapsulates the fixed-prompt drain logic with epoch
    /// tracking and tool-result continuation support (#1272, #1384).
    pub input_strategy: crate::application::loop_engine::input_strategy::SubInputStrategy<'a>,
}

impl<'a> SubAgentRun<'a> {
    fn freeze_request(&self, step_id: &sdk::RunStepId) -> crate::ports::ContextRequest {
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
            run_id: self.run_id.clone(),
            step_id: step_id.clone(),
            pending_messages: self.messages
                [self.committed_message_count + self.accepted_input.len()..]
                .to_vec(),
            system_prompt: crate::ports::SystemPromptSpec::new(&self.system),
            model_id: self.model_name_for_log.clone(),
            effective_reasoning: self.level,
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
            last_api_input_tokens: self.last_total_tokens,
            tool_schemas,
            tool_schema_tokens: context::compact::estimate_tool_schemas_tokens(&raw_tool_schemas),
            prev_system_tokens: None,
            prev_tool_schema_tokens: None,
        }
    }

    /// Runs a sub-agent through the same loop engine used by every agent run.
    pub async fn run_loop(mut self) -> AgentRunTerminal {
        let _signal_propagation = CancellationPropagationGuard::new(
            self.agent.ctx.cancellation(),
            self.runtime_cancellation.clone(),
        );
        let _binding = match tools::ToolExecutionContextBindingGuard::bind(
            self.tool_context_binding.clone(),
            self.agent.ctx.clone(),
        ) {
            Ok(binding) => binding,
            Err(error) => return AgentRunTerminal::Failed { error },
        };

        let input = crate::application::run_launcher::RunLaunchInput {
            run_id: self.run_id.clone(),
            spec: RunSpec::sub(self.role_name_for_log.clone(), self.timeout),
            parent_run_id: self.parent_run_id.clone(),
            cancel: self.runtime_cancellation.clone(),
        };
        let active_run = self.active_run.clone();

        let launch_result =
            crate::application::run_launcher::launch(input, active_run, &mut self).await;

        let loop_result = match launch_result {
            crate::application::run_launcher::RunLaunchResult::Terminal => Ok(()),
            crate::application::run_launcher::RunLaunchResult::Failed(error) => Err(error),
        };

        // A normal terminal path is recorded by `emit` from the authoritative
        // RunDomainEvent. Keep an infrastructure fallback so finalization still
        // runs if the engine itself cannot finish a transition.
        let terminal = self
            .terminal
            .take()
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
            turns: self.turn_count,
            duration: self.start_time.elapsed(),
            role: Some(self.role_name_for_log.clone()),
            model: self.model_name_for_log.clone(),
        };
        let output = terminal.output();
        finalize_sub_agent(
            &outcome,
            &self.hook_port,
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
        let msg_tokens = context::compact::estimate_messages_tokens(&self.messages);
        (self.progress)(
            Some(turn_number),
            &format!(
                "Agent turn {}, messages: {}, est_tokens: {}",
                turn_number,
                self.messages.len(),
                msg_tokens
            ),
        );
    }

    fn log_input(&self, system_blocks: &[RequestSystemBlock], tool_schemas: &[serde_json::Value]) {
        log_llm_input(
            &self.messages,
            self.committed_message_count,
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

    fn log_output(&self, resp: &InvocationResponse) {
        log_llm_output_and_tool_calls(
            &self.binding.model.provider,
            resp,
            &[],
            self.start_time.elapsed().as_secs_f64(),
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
}

fn terminal_from_domain_event(event: &RunDomainEvent) -> Option<AgentRunTerminal> {
    match event {
        RunDomainEvent::Completed { result, .. } => Some(AgentRunTerminal::Completed {
            result: result.clone(),
        }),
        RunDomainEvent::Failed { error, .. } => Some(AgentRunTerminal::Failed {
            error: error.clone(),
        }),
        RunDomainEvent::Cancelled { .. } | RunDomainEvent::Terminated { .. } => {
            Some(AgentRunTerminal::Cancelled)
        }
        RunDomainEvent::Transitioned { .. }
        | RunDomainEvent::Started { .. }
        | RunDomainEvent::StepStarted { .. }
        | RunDomainEvent::StepCompleted { .. }
        | RunDomainEvent::StepCancellationRequested { .. }
        | RunDomainEvent::StepFinalizationStarted { .. }
        | RunDomainEvent::StepCancelled { .. }
        | RunDomainEvent::DrainingInput { .. }
        | RunDomainEvent::TerminationRequested { .. }
        | RunDomainEvent::CancellationRequested { .. }
        | RunDomainEvent::AwaitingUser { .. }
        | RunDomainEvent::Resumed { .. }
        | RunDomainEvent::StuckDetected { .. } => None,
    }
}

fn should_complete_after_model_response(has_no_tool_calls: bool) -> bool {
    has_no_tool_calls
}

#[async_trait]
impl RunLoopPort for SubAgentRun<'_> {
    fn freeze_step(
        &mut self,
        step_id: &sdk::RunStepId,
        _inputs: &[crate::application::loop_engine::LoopInput],
    ) {
        self.accepted_input = if self.committed_message_count == 0 {
            self.messages
                .first()
                .filter(|message| message.role == share::message::Role::User)
                .cloned()
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        self.context_request = Some(self.freeze_request(step_id));
        self.context_window = None;
    }

    async fn accept_step_input(&mut self, step_id: &sdk::RunStepId) -> Result<(), LoopEngineError> {
        let request = self
            .context_request
            .as_ref()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        debug_assert_eq!(&request.step_id, step_id);
        if self.accepted_input.is_empty() {
            return Ok(());
        }
        self.context
            .append_accepted_input(request, self.accepted_input.clone())
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        Ok(())
    }

    /// #1272: Delegates to [`SubInputStrategy::drain_input`].
    async fn drain_input(
        &mut self,
        expected_epoch: crate::application::loop_engine::DrainEpoch,
    ) -> Result<crate::application::loop_engine::DrainOutcome, LoopEngineError> {
        use crate::application::loop_engine::input_strategy::InputStrategy;
        self.input_strategy.drain_input(expected_epoch).await
    }

    /// #1280: Delegates to [`SubInputStrategy::await_user_input`].
    async fn await_user_input(
        &mut self,
        _expected_epoch: crate::application::loop_engine::DrainEpoch,
    ) -> Result<crate::application::loop_engine::DrainOutcome, LoopEngineError> {
        use crate::application::loop_engine::input_strategy::InputStrategy;
        self.input_strategy.await_user_input(_expected_epoch).await
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        let (needed, window) =
            crate::application::loop_engine::shared::needs_compaction_with_window(
                self.context_request.as_ref(),
                &self.context,
            )
            .await?;
        self.context_window = Some(window);
        Ok(needed)
    }

    async fn compact(
        &mut self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), LoopEngineError> {
        let request = self
            .context_request
            .as_ref()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        let source_revision = self
            .context_window
            .as_ref()
            .map(|window| window.backing_revision)
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        let outcome = self
            .context
            .compact(request, source_revision)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        crate::application::context_coordination::apply_automatic_compact_outcome(
            &outcome,
            &mut self.last_total_tokens,
            &mut self.context_window,
        );
        Ok(())
    }

    async fn invoke_model(
        &mut self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        use crate::application::loop_engine::StepTokenUsage;
        self.turn_count += 1;
        let turn_number = self.turn_count;
        logging::within(
            logging::LogContextPatch {
                turn: logging::FieldPatch::Set(turn_number),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
            async move {
                self.progress_turn_start(turn_number);
                (self.log_request_messages)(turn_number, &self.messages);

                let window = if let Some(window) = self.context_window.clone() {
                    Some(window)
                } else if let Some(request) = &self.context_request {
                    Some(
                        self.context
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
                    .unwrap_or_else(|| messages_for_llm(&self.messages));
                let effective_blocks = window
                    .as_ref()
                    .map(|window| {
                        window
                            .system_blocks
                            .iter()
                            .map(|block| {
                                if block.cache_break {
                                    debug_assert!(
                                        block.cacheable,
                                        "cache breakpoint 必须位于可缓存前缀"
                                    );
                                    RequestSystemBlock::Cacheable(block.content.clone())
                                } else {
                                    RequestSystemBlock::Text(block.content.clone())
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let effective_tools = window
                    .as_ref()
                    .map(|window| window.tool_schemas.clone())
                    .unwrap_or_default();
                let raw_tool_schemas = effective_tools
                    .iter()
                    .map(|schema| {
                        serde_json::json!({
                            "name": schema.name,
                            "description": schema.description,
                            "input_schema": schema.input_schema,
                        })
                    })
                    .collect::<Vec<_>>();
                self.log_input(&effective_blocks, &raw_tool_schemas);
                self.context_window = window;
                let mut coordinator =
                    crate::application::model_invocation::ModelInvocationCoordinator::new();
                let resp = loop {
                    // A retry is a fresh provider request and therefore gets a fresh request id.
                    let request_context = sub_request_log_context(
                        &logging::capture(),
                        &self.model_name_for_log,
                        &self.binding.model.provider,
                        &self.role_name_for_log,
                    );
                    let response = logging::instrument(request_context, async {
                        let mut reducer =
                            crate::application::main_loop::looping::InvocationEventReducer::new(
                                SubAgentEventSink,
                            );
                        let provider = self.binding.provider.clone();
                        let model = self.binding.model.clone();
                        let max_tokens = self.max_tokens;
                        let level = self.level;
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
                                        .pull_stream(stream, &cancellation, false, |event| {
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
                        Err((error, visible_delta)) => match coordinator
                            .handle_failure(&error, visible_delta, &self.runtime_cancellation)
                            .await
                        {
                            crate::application::model_invocation::RetryStep::Retry {
                                attempt,
                                delay,
                            } => {
                                log::info!(
                                    target: crate::LOG_TARGET,
                                    "sub-agent model invocation retrying: attempt={} delay_ms={}",
                                    attempt,
                                    delay.as_millis(),
                                );
                            }
                            crate::application::model_invocation::RetryStep::Cancelled => {
                                self.runtime_cancellation.cancel();
                                return Err(LoopEngineError::Cancelled);
                            }
                            crate::application::model_invocation::RetryStep::Compact => {
                                return Err(LoopEngineError::NeedsCompaction(error.to_string()));
                            }
                            crate::application::model_invocation::RetryStep::Fail => {
                                return Err(LoopEngineError::Adapter(error.to_string()));
                            }
                        },
                    }
                };

                self.last_total_tokens = Some(
                    crate::application::token_usage::normalized_total_tokens(&resp.usage),
                );
                self.progress_api_ok(turn_number, &resp);

                let usage = StepTokenUsage {
                    input_tokens: resp.usage.input_tokens.unwrap_or(0) as u64,
                    output_tokens: resp.usage.output_tokens.unwrap_or(0) as u64,
                    cached_tokens: resp.usage.cache_read_tokens.map(u64::from).unwrap_or(0),
                    cache_creation_tokens: resp
                        .usage
                        .cache_write_tokens
                        .map(u64::from)
                        .unwrap_or(0),
                    reasoning_tokens: resp.usage.reasoning_tokens.map(u64::from).unwrap_or(0),
                    total_tokens: crate::application::token_usage::normalized_total_tokens(
                        &resp.usage,
                    ),
                    context_window: self.ctx_context_size as u64,
                    est_system_tokens: self
                        .context_window
                        .as_ref()
                        .map_or(0, |window| window.token_estimation.system_tokens),
                    est_tool_tokens: self
                        .context_window
                        .as_ref()
                        .map_or(0, |window| window.token_estimation.tool_schema_tokens),
                    est_message_tokens: self
                        .context_window
                        .as_ref()
                        .map_or(0, |window| window.token_estimation.message_tokens),
                    stop_reason: format!("{:?}", resp.stop_reason).to_lowercase(),
                };

                self.messages.push(resp.assistant_message.clone());
                self.log_output(&resp);
                self.send_text_progress(turn_number, &resp);

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                if resp.stop_reason == StopReason::MaxOutputTokens {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "turn {}: 模型响应触发 max_tokens 限制，注入分块提示",
                        turn_number,
                    );
                    self.messages.push(Message::user(
                        "[系统提示] 你的上一次响应触达了 max_tokens 限制，输出被截断。\
                 请基于已有内容继续，或用更紧凑的方式重新组织响应：\
                 大文件改用 Edit 分块写入（每次 < 12k 字符），\
                 长命令用 Bash heredoc 分段执行。\
                 不要重复已输出的内容，直接从截断点继续。"
                            .to_string(),
                    ));
                    if tool_calls.is_empty() {
                        // Preserve the old retry path: a text-only truncation did not
                        // trigger compaction before asking the model to continue.
                        self.last_total_tokens = None;
                        // An empty tool phase advances the shared state machine while
                        // retaining the old behavior of retrying a truncated response.
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
                    if text.trim().is_empty() {
                        let block_types: Vec<&str> = resp
                            .assistant_message
                            .content
                            .iter()
                            .map(|b| match b {
                                share::message::ContentBlock::Text { .. } => "text",
                                share::message::ContentBlock::Image { .. } => "image",
                                share::message::ContentBlock::ToolUse { .. } => "tool_use",
                                share::message::ContentBlock::ToolResult { .. } => "tool_result",
                                share::message::ContentBlock::Thinking { .. } => "thinking",
                            })
                            .collect();
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "{}",
                            serde_json::json!({
                                "event_type": "subagent_empty_complete_text",
                                "block_count": resp.assistant_message.content.len(),
                                "block_types": block_types,
                                "has_thinking": resp.assistant_message.content.iter().any(|b| matches!(b, share::message::ContentBlock::Thinking { .. })),
                                "text_len": text.len(),
                                "stop_reason": format!("{:?}", resp.stop_reason),
                                "role": self.role_name_for_log,
                            })
                        );
                    }
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

    async fn finalize_step(&mut self, step_id: &sdk::RunStepId) -> Result<(), LoopEngineError> {
        let (Some(request), Some(window)) = (&self.context_request, &self.context_window) else {
            return Ok(());
        };
        debug_assert_eq!(&request.step_id, step_id);
        let messages =
            self.messages[self.committed_message_count + self.accepted_input.len()..].to_vec();
        self.context
            .append_finalized(
                request,
                step_id.clone(),
                window.backing_revision,
                crate::ports::FinalizeCause::Completed,
                messages,
                vec![],
                self.last_total_tokens,
            )
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.committed_message_count = self.messages.len();
        Ok(())
    }

    async fn finalize_cancelled_step(
        &mut self,
        step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        let (Some(request), Some(window)) = (&self.context_request, &self.context_window) else {
            return Ok(());
        };
        debug_assert_eq!(&request.step_id, step_id);
        let messages =
            self.messages[self.committed_message_count + self.accepted_input.len()..].to_vec();
        self.context
            .append_finalized(
                request,
                step_id.clone(),
                window.backing_revision,
                crate::ports::FinalizeCause::UserCancelledStep,
                messages,
                vec![],
                self.last_total_tokens,
            )
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.committed_message_count = self.messages.len();
        Ok(())
    }

    async fn execute_tools(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(crate::application::subagent::ToolCall, ToolGuardDecision)],
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        let turn_number = self.turn_count;
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
                    self.policy.as_ref(),
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
                    &mut self.messages,
                    results,
                    &self.session_id,
                )
                .await;
                // #1384: Mark that tool results are pending so drain_input
                // returns InternalContinuation instead of EmptyAndSealed.
                self.input_strategy.has_tool_results_pending = true;
                Ok(if fuse_bypassed.is_empty() {
                    ToolStep::Continue
                } else {
                    ToolStep::ContinueWithFuseBypass(fuse_bypassed)
                })
            },
        )
        .await
    }

    async fn on_stuck(
        &mut self,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        (self.progress)(Some(self.turn_count), &format!("StuckGuard: {decision:?}"));
        Ok(())
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_cancellation(run_id)
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            if let Some(terminal) = terminal_from_domain_event(&event) {
                match &terminal {
                    AgentRunTerminal::Completed { .. } => {
                        (self.progress)(Some(self.turn_count), "Agent completed");
                    }
                    AgentRunTerminal::Failed { error } => {
                        (self.progress)(Some(self.turn_count), &format!("Agent error: {error}"));
                    }
                    AgentRunTerminal::Cancelled => {
                        (self.progress)(Some(self.turn_count), "Agent cancelled by user");
                    }
                }
                self.terminal = Some(terminal);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{messages_for_llm, terminal_from_domain_event};
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
