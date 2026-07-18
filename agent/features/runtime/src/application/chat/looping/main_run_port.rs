use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::application::agent::{Agent, ToolCall};
use crate::application::chat::looping::compact::auto_compact;
use crate::application::chat::looping::compact_outcome::apply_compact_outcome;
use crate::application::chat::looping::finalize::{
    finish_completed_loop, run_stop_hook_before_finish,
};
use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::application::chat::looping::loop_phases::build_api_messages;
use crate::application::chat::looping::memory_inject::build_memory_block;
use crate::application::chat::looping::post_batch::run_post_tool_batch;
use crate::application::chat::looping::reflection::{run_reflection, should_run_turn_reflection};
use crate::application::chat::looping::stream_handler::{
    should_emit_model_stream_waiting, InvocationEventReducer,
};
use crate::application::chat::looping::task_reminder::TaskReminderState;
use crate::application::chat::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::application::chat::looping::{
    ChatEventSink, InputEventDrainPort, PendingInputBuffer, QueueDrainPort, RuntimeStreamEvent,
    RuntimeTurnContext,
};
use crate::application::loop_engine::{
    split_input_events, LoopEngineError, LoopInput, ModelStep, RunLoopPort, ToolGuardDecision,
    ToolStep,
};
use crate::domain::agent_run::RunDomainEvent;
use crate::LOG_TARGET;
use context::session::ChatChain;
use workflow::api::{ReasoningPort, ReasoningSignal};

/// Main-chat adapter for the shared run loop.
///
/// It owns no lifecycle state machine. `Run` is the only per-run state machine; this adapter
/// projects its domain events and bridges the existing provider/tool/compact/hook helpers.
#[allow(clippy::too_many_arguments)]
pub(crate) struct MainRunPort<'a, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub(crate) sink: &'a S,
    pub(crate) queue: &'a Q,
    pub(crate) input_events: &'a I,
    pub(crate) client: &'a Arc<provider::LlmClient>,
    pub(crate) registry: &'a Arc<tools::ToolRegistry>,
    pub(crate) system_blocks: &'a [provider::SystemBlock],
    pub(crate) system_prompt_text: &'a str,
    pub(crate) user_context: &'a str,
    pub(crate) chain: &'a mut ChatChain,
    pub(crate) context_size: usize,
    pub(crate) workspace: &'a project::WorkspaceViews,
    pub(crate) session_id: &'a str,
    pub(crate) read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(crate) session_reminders: &'a Arc<std::sync::Mutex<tools::SessionReminders>>,
    pub(crate) agent_runner: &'a Option<Arc<dyn tools::AgentRunner>>,
    pub(crate) tool_result_materializer:
        &'a crate::application::tool_result_materialization::ToolResultMaterializer,
    pub(crate) allow_all: bool,
    /// Runtime/Tool 日常状态唯一来源（#889 low-privilege 端口）。
    pub(crate) task_access: &'a Arc<dyn task::TaskAccess>,
    pub(crate) max_tool_concurrency: usize,
    pub(crate) agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub(crate) hook_runner: &'a hook::api::HookRunner,
    pub(crate) memory_config: &'a share::config::MemoryConfig,
    pub(crate) memory: &'a Arc<dyn memory::MemoryPort>,
    pub(crate) language: &'a str,
    pub(crate) frozen_chats: &'a Arc<std::sync::Mutex<Vec<context::session::ChatSegment>>>,
    pub(crate) active_summary: &'a mut Option<String>,
    pub(crate) active_summary_arc: &'a Arc<std::sync::Mutex<Option<String>>>,
    pub(crate) reasoning: &'a dyn ReasoningPort,
    pub(crate) save_chain: &'a crate::application::chat::looping::loop_context::SaveChainFn,
    pub(crate) pending_input: &'a mut PendingInputBuffer,
    pub(crate) deferred_user_inputs: &'a mut VecDeque<sdk::ChatInputEvent>,
    pub(crate) cancel: CancellationToken,
    pub(crate) run_id: sdk::RunId,
    pub(crate) active_run: &'a dyn crate::domain::agent_run::ActiveRunPort,
    pub(crate) turn_count: usize,
    pub(crate) segment_id: &'a str,
    pub(crate) turn_context: RuntimeTurnContext,
    pub(crate) rollback_chain: ChatChain,
    pub(crate) rollback_frozen_chats: Vec<context::session::ChatSegment>,
    pub(crate) rollback_active_summary: Option<String>,
    pub(crate) memory_cwd: PathBuf,
    pub(crate) last_total_tokens: &'a mut Option<u64>,
    pub(crate) task_reminder_state: &'a mut TaskReminderState,
    pub(crate) tool_identity:
        &'a crate::application::chat::looping::tool_identity::ToolIdentityRegistry,
    pub(crate) started_at: Instant,
}

impl<S, Q, I> MainRunPort<'_, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    /// 实时从 Project-owned `WorkspaceRead` 读取 `workspace_root`，避免 turn 内
    /// 切换 worktree 后使用过时路径。
    fn current_cwd(&self) -> PathBuf {
        self.workspace.read().current_workspace_root()
    }

    async fn queue_busy_event(&mut self, event: sdk::ChatInputEvent) {
        match event {
            sdk::ChatInputEvent::UserMessage { .. } => {
                self.deferred_user_inputs.push_back(event);
                let queued = self
                    .deferred_user_inputs
                    .iter()
                    .filter_map(|event| match event {
                        sdk::ChatInputEvent::UserMessage { id, text, .. } => {
                            Some((id.clone(), Message::user(text.clone())))
                        }
                        _ => None,
                    })
                    .collect();
                self.sink
                    .send_event(RuntimeStreamEvent::UserMessagesQueued { queued })
                    .await;
            }
            sdk::ChatInputEvent::WithdrawAll => {
                let texts = self
                    .deferred_user_inputs
                    .drain(..)
                    .filter_map(|event| match event {
                        sdk::ChatInputEvent::UserMessage { text, .. } => Some(text),
                        _ => None,
                    })
                    .collect();
                self.sink
                    .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                    .await;
            }
            // Commands are retained for the session idle gate. They never enter model context.
            other => self.pending_input.push(other),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_agent<'b>(
        registry: &'b Arc<tools::ToolRegistry>,
        agent_runner: &Option<Arc<dyn tools::AgentRunner>>,
        memory: &Arc<dyn memory::MemoryPort>,
        language: &str,
        allow_all: bool,
        workspace: &project::WorkspaceViews,
        cancel: &CancellationToken,
        read_files: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
        session_reminders: &Arc<std::sync::Mutex<tools::SessionReminders>>,
        max_tool_concurrency: usize,
        agent_semaphore: &Arc<tokio::sync::Semaphore>,
        session_id: &str,
        run_id: &sdk::RunId,
    ) -> Agent<'b> {
        Agent {
            registry,
            ctx: tools::ToolExecutionContext::new(
                tools::ExecutionScope::builder(
                    run_id.to_string(),
                    workspace.read().workspace_id(),
                    workspace.read().current_workspace_root(),
                )
                .build(),
                tools::ToolExecutionPorts::new(
                    crate::application::tool_execution_adapters::cancellation(cancel.clone()),
                    crate::application::tool_execution_adapters::RuntimeWorkspaceAccess::new(
                        workspace.clone(),
                    )
                    .read_access(),
                    Arc::new(tools::MutexReadSet(read_files.clone())),
                    Arc::new(tools::FixedPlanMode(None)),
                    memory.clone(),
                    Arc::new(tools::FixedGuidance {
                        language: language.to_string(),
                        allow_all,
                    }),
                )
                .with_memory_context(
                    Some(session_id.to_string()),
                    Some(session_reminders.clone()),
                )
                .with_agent(agent_runner.clone())
                .with_catalog(Some(registry.clone() as Arc<dyn tools::CatalogQuery>)),
            ),
            max_tool_concurrency,
            agent_semaphore: agent_semaphore.clone(),
            workspace_persist: workspace.persist(),
            runtime_cancellation: cancel.clone(),
        }
    }

    fn outcome(&self, status: AgentRunStatus) -> AgentRunOutcome {
        AgentRunOutcome {
            status,
            turns: self.turn_count,
            duration: self.started_at.elapsed(),
            role: None,
            model: self.client.model_name().to_string(),
        }
    }

    async fn project_done(&self, status: AgentRunStatus) {
        let outcome = self.outcome(status);
        log_agent_outcome(&outcome, self.session_id);
        finish_completed_loop(&outcome, self.sink, &self.turn_context, &**self.task_access).await;
    }

    async fn rollback_cancelled(&mut self) {
        *self.chain = self.rollback_chain.clone();
        if let Ok(mut frozen) = self.frozen_chats.lock() {
            *frozen = self.rollback_frozen_chats.clone();
        }
        *self.active_summary = self.rollback_active_summary.clone();
        if let Ok(mut summary) = self.active_summary_arc.lock() {
            *summary = self.rollback_active_summary.clone();
        }
        self.sink
            .send_event(RuntimeStreamEvent::CompactRollback {
                messages: self.chain.messages_flat(),
            })
            .await;
        if let Err(error) = (self.save_chain)(self.chain).await {
            log::error!(target: LOG_TARGET, "cancel rollback save_chain failed: {error}");
        }
        self.sink
            .send_event(RuntimeStreamEvent::Cancelled {
                context: self.turn_context.clone(),
            })
            .await;
    }

    async fn compact_impl(&mut self) {
        let cleared = context::compact::microcompact_chain(self.chain);
        if cleared > 0 {
            self.sink
                .send_event(RuntimeStreamEvent::MicrocompactDone {
                    messages: self.chain.messages_flat(),
                    cleared_count: cleared,
                })
                .await;
        }
        if let Some(outcome) = auto_compact(
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            self.turn_count,
            &self.chain.messages_flat(),
            self.active_summary.as_deref(),
            self.system_prompt_text,
            self.context_size,
            self.memory_config,
            self.memory.as_ref(),
            &crate::application::chat::looping::reflection::REFLECTION_ENGINE,
            self.client,
            self.language,
            &self.current_cwd(),
            &self.cancel,
        )
        .await
        {
            apply_compact_outcome(
                self.sink,
                outcome,
                self.chain,
                self.frozen_chats,
                self.active_summary,
                self.active_summary_arc,
            )
            .await;
            // compact 后清空最近 usage；只有下一次 Provider 响应才能再次触发。
            *self.last_total_tokens = None;
        }
    }

    async fn invoke_model_impl(
        &mut self,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.task_reminder_state
            .update_from_messages(self.turn_count as u64, &self.chain.messages_flat());
        let messages_for_api: Vec<Message> = build_api_messages(
            self.user_context,
            self.language,
            self.task_reminder_state,
            self.turn_count as u64,
            &**self.task_access,
            &self.chain.messages_flat(),
        )
        .await;
        let tool_schemas = self.registry.schemas_for(self.language);
        let mut effective_system_blocks = self.system_blocks.to_vec();
        if self.memory_config.enabled && self.memory_config.inject_count > 0 {
            if let Some(block) =
                build_memory_block(&self.memory_cwd, self.memory_config.inject_count)
            {
                effective_system_blocks.push(block);
            }
        }
        if let Some(summary) = self.active_summary.clone() {
            effective_system_blocks.push(provider::SystemBlock {
                block_type: "text".to_string(),
                text: format!("<compact-summary>\n{summary}\n</compact-summary>"),
                cache_control: None,
            });
        }
        log_llm_input(
            &messages_for_api,
            self.chain.message_count(),
            &effective_system_blocks,
            &tool_schemas,
        );
        let requested_reasoning = self.reasoning.current_requested_level();
        let invocation_scope = self
            .client
            .invocation_scope(
                self.client.default_scope().model(),
                None,
                requested_reasoning,
            )
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;

        logging::set_current_model(self.client.model_name().to_string());
        logging::set_current_provider(self.client.provider_name().to_string());
        logging::set_current_role("default".to_string());
        logging::set_current_request_id(uuid::Uuid::now_v7().to_string());

        let api_start = Instant::now();
        let mut coordinator =
            crate::application::model_invocation::ModelInvocationCoordinator::new();
        let resp = loop {
            let mut reducer = InvocationEventReducer::with_tool_identity(
                self.sink.clone(),
                self.tool_identity.clone(),
                self.turn_context.clone(),
            );
            let response = {
                let progress_handle = reducer.progress_handle();
                let stream_cancel = self.cancel.clone();
                let invocation_fut = async {
                    let stream = self
                        .client
                        .invocation_stream(
                            &invocation_scope,
                            &effective_system_blocks,
                            &messages_for_api,
                            &tool_schemas,
                            &stream_cancel,
                        )
                        .await
                        .map_err(|error| (error, false))?;
                    coordinator
                        .pull_stream(stream, &stream_cancel, true, |event| reducer.apply(event))
                        .await
                };
                let waiting_sink = self.sink.clone();
                let waiting_context = self.turn_context.clone();
                let request_started_at = tokio::time::Instant::now();
                let waiting_task = tokio::spawn(async move {
                    let mut next = request_started_at + Duration::from_secs(10);
                    let mut last_version = None;
                    loop {
                        tokio::time::sleep_until(next).await;
                        let snapshot = progress_handle.lock().unwrap().snapshot();
                        if should_emit_model_stream_waiting(last_version, &snapshot) {
                            waiting_sink.try_send_event(RuntimeStreamEvent::ModelStreamWaiting {
                                context: waiting_context.clone(),
                                elapsed_secs: request_started_at.elapsed().as_secs(),
                                phase: snapshot.phase.to_string(),
                            });
                        }
                        last_version = Some(snapshot.visible_progress_version);
                        next += Duration::from_secs(10);
                    }
                });
                tokio::pin!(invocation_fut);
                let result = loop {
                    tokio::select! {
                        response = &mut invocation_fut => break response,
                        event = self.input_events.recv_next_input() => {
                            if let Some(event) = event {
                                self.queue_busy_event(event).await;
                            }
                        }
                    }
                };
                waiting_task.abort();
                result
            };
            match response {
                Ok((response, _)) => break response,
                Err((error, _)) if error.is_cancelled() || self.cancel.is_cancelled() => {
                    return Err(LoopEngineError::Cancelled);
                }
                Err((error, visible_delta)) => match coordinator
                    .handle_failure(&error, visible_delta, &self.cancel)
                    .await
                {
                    crate::application::model_invocation::RetryStep::Retry { attempt, delay } => {
                        self.sink
                            .try_send_event(RuntimeStreamEvent::ModelInvocationRetrying {
                                context: self.turn_context.clone(),
                                attempt,
                                delay,
                            });
                    }
                    crate::application::model_invocation::RetryStep::Cancelled => {
                        return Err(LoopEngineError::Cancelled);
                    }
                    crate::application::model_invocation::RetryStep::Compact
                    | crate::application::model_invocation::RetryStep::Fail => {
                        return Err(LoopEngineError::Adapter(error.to_string()));
                    }
                },
            }
        };
        let api_elapsed = api_start.elapsed().as_secs_f64();

        // Poll the non-blocking legacy queue at the model boundary. Busy user input is kept for a
        // fresh Run and never appended to this Run's model context.
        if let Some(queued) = self.queue.drain_queued_input().await {
            for text in queued {
                self.queue_busy_event(sdk::ChatInputEvent::classify_text(text, Vec::new()))
                    .await;
            }
        }

        *self.last_total_tokens = Some(crate::application::token_usage::normalized_total_tokens(
            &resp.usage,
        ));

        let token_usage = crate::application::loop_engine::StepTokenUsage {
            input_tokens: resp.usage.input_tokens as u64,
            output_tokens: resp.usage.output_tokens as u64,
            cached_tokens: resp.usage.cached_tokens.map(u64::from).unwrap_or(0),
            cache_creation_tokens: resp.usage.cache_creation_tokens.map(u64::from).unwrap_or(0),
            reasoning_tokens: resp.usage.reasoning_tokens.map(u64::from).unwrap_or(0),
            total_tokens: crate::application::token_usage::normalized_total_tokens(&resp.usage),
            context_window: self.context_size as u64,
            est_system_tokens: effective_system_blocks
                .iter()
                .map(|b| context::compact::estimate_tokens(&b.text))
                .sum(),
            est_tool_tokens: context::compact::estimate_tool_schemas_tokens(&tool_schemas),
            est_message_tokens: context::compact::estimate_messages_tokens(&messages_for_api),
            stop_reason: format!("{:?}", resp.stop_reason).to_lowercase(),
        };

        self.sink
            .send_event(RuntimeStreamEvent::Usage {
                input: resp.usage.input_tokens,
                output: resp.usage.output_tokens,
                last_input: resp.usage.input_tokens,
                elapsed_secs: api_elapsed,
            })
            .await;
        self.chain
            .push(resp.assistant_message.clone(), self.segment_id);
        self.sink
            .send_event(RuntimeStreamEvent::TurnStarted {
                messages: self.chain.messages_flat(),
            })
            .await;

        let calls = Agent::extract_tool_calls_with_ids(&resp.assistant_message, |provider_id| {
            self.tool_identity.runtime_id_for_provider(provider_id)
        });
        log_llm_output_and_tool_calls(self.client.provider_name(), &resp, &calls, api_elapsed);
        if !calls.is_empty() {
            return Ok((
                ModelStep::Tools {
                    text: resp.assistant_message.text_content(),
                    calls,
                },
                token_usage,
            ));
        }

        let observation = self.reasoning.observe(ReasoningSignal::TextOnly);
        if observation.changed() {
            self.sink
                .send_event(RuntimeStreamEvent::GraphPhaseChanged {
                    node: observation.current,
                    effort: observation.requested,
                    prev: observation.previous,
                })
                .await;
        }
        if should_run_turn_reflection(
            self.memory_config,
            self.turn_count,
            !calls.is_empty(),
            &resp.stop_reason,
            false,
        ) {
            if let Some(text) = run_reflection(
                self.memory_config,
                self.turn_count,
                &self.chain.messages_flat(),
                self.client,
                self.system_prompt_text,
                self.language,
                self.memory.as_ref(),
                &crate::application::chat::looping::reflection::REFLECTION_ENGINE,
            )
            .await
            {
                self.sink
                    .send_event(RuntimeStreamEvent::SystemMessage(text))
                    .await;
            }
        }

        let outcome = self.outcome(AgentRunStatus::Completed);
        if let Some(feedback) = run_stop_hook_before_finish(
            &outcome,
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            self.session_id,
            self.language,
            &self.current_cwd(),
            &self.cancel,
        )
        .await
        {
            self.chain.push(
                Message::system_generated_user(format!(
                    "<system-reminder>\n{feedback}\n</system-reminder>"
                )),
                self.segment_id,
            );
            self.sink
                .send_event(RuntimeStreamEvent::StopHookBlocked {
                    messages: self.chain.messages_flat(),
                })
                .await;
            return Ok((
                ModelStep::StopHookBlocked {
                    text: resp.assistant_message.text_content(),
                },
                token_usage,
            ));
        }
        Ok((
            ModelStep::Complete {
                text: resp.assistant_message.text_content(),
            },
            token_usage,
        ))
    }
}

#[async_trait]
impl<S, Q, I> RunLoopPort for MainRunPort<'_, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn drain_input(&mut self) -> Result<Vec<LoopInput>, LoopEngineError> {
        let mut events = self.input_events.drain_input_events().await;
        if let Some(queued) = self.queue.drain_queued_input().await {
            events.extend(
                queued
                    .into_iter()
                    .map(|text| sdk::ChatInputEvent::classify_text(text, Vec::new())),
            );
        }
        let batch = split_input_events(events.clone());
        let inputs = batch
            .user_inputs
            .into_iter()
            .map(|input| LoopInput { text: input.text })
            .collect();
        for event in events {
            self.queue_busy_event(event).await;
        }
        Ok(inputs)
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        let needed = super::super::compact::should_compact_now(
            *self.last_total_tokens,
            self.context_size,
            self.chain.message_count(),
        );
        Ok(needed)
    }

    async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        // The existing compact helper is not cancellation-aware. Always return control to the
        // engine; it performs the canonical post-compact cancellation transition and emits both
        // CancellationRequested and Cancelled from the Run aggregate.
        self.compact_impl().await;
        Ok(())
    }

    async fn invoke_model(
        &mut self,
        _cancel: &CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.invoke_model_impl().await
    }

    async fn execute_tools(
        &mut self,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        if cancel.is_cancelled() {
            return Err(LoopEngineError::Cancelled);
        }
        if calls.is_empty() {
            return Ok(ToolStep::Continue);
        }
        let raw_calls: Vec<_> = calls.iter().map(|(call, _)| call.clone()).collect();
        let agent = Self::make_agent(
            self.registry,
            self.agent_runner,
            self.memory,
            self.language,
            self.allow_all,
            self.workspace,
            &self.cancel,
            self.read_files,
            self.session_reminders,
            self.max_tool_concurrency,
            self.agent_semaphore,
            self.session_id,
            &self.run_id,
        );
        let all_results = execute_tool_round(
            &self.turn_context,
            &raw_calls,
            self.registry,
            self.allow_all,
            &agent,
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            cancel,
            self.language,
            &self.current_cwd(),
            calls,
        )
        .await;
        if cancel.is_cancelled() {
            return Err(LoopEngineError::Cancelled);
        }

        let metadata: HashMap<&str, (Option<&str>, Option<&str>)> = raw_calls
            .iter()
            .map(|call| {
                let command = (call.name == "Bash")
                    .then(|| call.input.get("command").and_then(|value| value.as_str()))
                    .flatten();
                let phase = call.input.get("phase").and_then(|value| value.as_str());
                (call.provider_id.as_str(), (command, phase))
            })
            .collect();
        for result in &all_results {
            let (command, phase) = metadata
                .get(result.provider_id.as_str())
                .copied()
                .unwrap_or((None, None));
            let observation = self.reasoning.observe(ReasoningSignal::ToolCompleted {
                tool_name: result.tool_name.clone(),
                bash_command: command.map(str::to_string),
                is_error: result.outcome.is_error,
                declared_phase: phase.map(str::to_string),
            });
            if observation.changed() {
                self.sink
                    .send_event(RuntimeStreamEvent::GraphPhaseChanged {
                        node: observation.current,
                        effort: observation.requested,
                        prev: observation.previous,
                    })
                    .await;
            }
        }
        let has_task_mutation = all_results.iter().any(|result| {
            crate::application::chat::looping::events::is_task_store_mutation(&result.tool_name)
        });
        self.chain.push(
            tool_results_for_api(self.tool_result_materializer, all_results, self.session_id).await,
            self.segment_id,
        );
        self.sink
            .send_event(RuntimeStreamEvent::PostToolExecutionSync {
                messages: self.chain.messages_flat(),
            })
            .await;
        if has_task_mutation {
            let snapshot = crate::application::chat::looping::task_snapshot::build_task_snapshot(
                &**self.task_access,
            );
            self.sink
                .send_event(RuntimeStreamEvent::TasksSnapshot {
                    tasks: Box::new(snapshot),
                })
                .await;
        }
        run_post_tool_batch(
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            &agent.runtime_cancellation,
            self.turn_count,
            &self.current_cwd(),
        )
        .await;
        Ok(ToolStep::Continue)
    }

    async fn on_stuck(
        &mut self,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        let _ = decision;
        Ok(())
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run.claim_cancellation(run_id)
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            match event {
                RunDomainEvent::Completed { .. } => {
                    if let Err(error) = (self.save_chain)(self.chain).await {
                        log::error!(target: LOG_TARGET, "turn-level save_chain failed: {error}");
                    }
                    self.project_done(AgentRunStatus::Completed).await;
                }
                RunDomainEvent::Failed { error, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::ApiError {
                            messages: self.chain.messages_flat(),
                            error: error.clone(),
                        })
                        .await;
                    if let Err(save_error) = (self.save_chain)(self.chain).await {
                        log::error!(target: LOG_TARGET, "api-error save_chain failed: {save_error}");
                    }
                    self.project_done(AgentRunStatus::ApiError(error)).await;
                }
                RunDomainEvent::Cancelled { run_id, .. } => {
                    self.rollback_cancelled().await;
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelled { run_id })
                        .await;
                }
                RunDomainEvent::Terminated { run_id, .. } => {
                    self.rollback_cancelled().await;
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelled { run_id })
                        .await;
                }
                RunDomainEvent::CancellationRequested { run_id, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelling { run_id })
                        .await;
                }
                RunDomainEvent::Started {
                    run_id,
                    parent_run_id,
                } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::RunStarted {
                            run_id,
                            parent_run_id,
                        })
                        .await;
                }
                RunDomainEvent::StuckDetected { reason, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::SystemMessage(format!(
                            "[StuckGuard: {reason}]"
                        )))
                        .await;
                }
                RunDomainEvent::Transitioned { .. }
                | RunDomainEvent::AwaitingUser { .. }
                | RunDomainEvent::Resumed { .. }
                | RunDomainEvent::StepStarted { .. }
                | RunDomainEvent::StepCompleted { .. }
                | RunDomainEvent::StepCancellationRequested { .. }
                | RunDomainEvent::StepFinalizationStarted { .. }
                | RunDomainEvent::StepCancelled { .. }
                | RunDomainEvent::DrainingInput { .. }
                | RunDomainEvent::TerminationRequested { .. } => {
                    self.sink.send_domain_event(event).await;
                }
            }
        }
        Ok(())
    }
}
