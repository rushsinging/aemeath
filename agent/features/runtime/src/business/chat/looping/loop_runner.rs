use crate::business::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::business::agent::Agent;
use crate::business::chat::looping::compact::auto_compact;
use crate::business::chat::looping::finalize::{
    finalize_main_loop, finish_completed_loop, run_stop_hook_before_finish,
};
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::business::chat::looping::post_batch::run_post_tool_batch;
use crate::business::chat::looping::reflection::{run_reflection, should_run_turn_reflection};
use crate::business::chat::looping::stall::StallDetector;
use crate::business::chat::looping::task_reminder::TaskReminderState;
use crate::business::chat::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::business::chat::looping::{
    ChatEventSink, ChatLoopFsm, ChatLoopState, ChatLoopTransition, GateDecision, GateKind,
    InputEventDrainPort, PendingInputBuffer, QueueDrainPort, RuntimeStreamEvent,
    RuntimeStreamHandler, RuntimeTurnContext,
};
use provider::api::StopReason;
use share::message::Message;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::api::ToolRegistry;

pub struct ChatLoopContext<S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub sink: S,
    pub queue: Q,
    pub input_events: I,
    pub client: Arc<provider::api::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<provider::api::SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub messages: Vec<Message>,
    pub context_size: usize,
    pub cwd: PathBuf,
    pub workspace: Arc<project::api::WorkspaceService>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<share::tool::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn tools::api::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub task_store: Arc<storage::api::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: hook::api::HookRunner,
    pub memory_config: share::config::MemoryConfig,
}

/// Background task: runs the agent loop and sends UI events via sink.
pub async fn process_chat_loop<S, Q, I>(ctx: ChatLoopContext<S, Q, I>)
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    let ChatLoopContext {
        sink,
        queue,
        input_events,
        ref client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        mut messages,
        context_size,
        cwd: _seed_cwd,
        workspace,
        session_id,
        read_files,
        session_reminders,
        agent_runner,
        allow_all,
        interrupted,
        cancel,
        task_store,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        hook_runner,
        memory_config,
    } = ctx;
    let hook_ui = HookUi::new(sink.clone());

    // workspace service 跨 chat 轮次持有：恢复 session 时已 restore 到正确位置，
    // 这里直接读取当前 root 作为 hook/日志的工作目录基准（忽略 seed cwd）。
    let cwd = project::api::WorkspaceRead::current_root(workspace.as_ref());
    let in_worktree = project::api::WorkspaceRead::in_worktree(workspace.as_ref());
    hook_runner.set_project_context(cwd.display().to_string(), in_worktree);
    log::info!(
        "chat loop hook runner ready: project_dir={} configured_events={}",
        hook_runner.project_dir(),
        hook_runner.hook_count()
    );
    let agent = Agent {
        registry: &registry,
        ctx: tools::api::ToolExecutionContext {
            cwd: cwd.clone(),
            workspace: workspace.clone(),
            cancel: cancel.clone(),
            read_files: read_files.clone(),
            agent_runner: agent_runner.clone(),
            session_reminders: Some(session_reminders.clone()),
            memory_config: memory_config.clone(),
            plan_mode: None,
            allow_all,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
            progress_tx: None,
            parent_session_id: Some(session_id.clone()),
        },
    };

    let messages_at_start = messages.len();
    let mut last_api_input_tokens: u64 = 0;
    let turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;
    let mut task_reminder_state = TaskReminderState::new();
    let mut stall_detector = StallDetector::new();
    let mut pending_input = PendingInputBuffer::default();
    let mut loop_fsm = ChatLoopFsm::default();
    let tool_identity = crate::business::chat::looping::tool_identity::ToolIdentityRegistry::new();
    let chat_id = format!("session-{session_id}");
    // 将 chat_id 同步到日志 context，影响 tool/audit/hook 等共享 sink 的 chat 字段
    logging::context::set_current_chat_id(chat_id.clone());
    loop {
        turn_count += 1;
        let turn_context = RuntimeTurnContext::new(chat_id.clone(), format!("turn-{turn_count}"));
        loop_fsm.transition(ChatLoopTransition::StartTurn);
        sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
            .await; // Refresh tool schemas each turn so dynamically registered MCP tools
                    // are visible to the LLM once the background connector finishes.
        let tool_schemas = registry.schemas();
        let tool_schema_tokens =
            crate::business::compact::estimate_tool_schemas_tokens(&tool_schemas);

        if interrupted.load(Ordering::Relaxed) {
            interrupted.store(false, Ordering::Relaxed);
            let outcome = drain_and_apply_gate(
                GateKind::BeforeFinish,
                &mut pending_input,
                &queue,
                &input_events,
                &sink,
                &mut messages,
            )
            .await;
            if outcome.decision == GateDecision::ContinueNextTurn {
                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                continue;
            }
            messages.truncate(messages_at_start);
            sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                .await;
            sink.send_event(RuntimeStreamEvent::Cancelled {
                context: turn_context.clone(),
            })
            .await;
            loop_fsm.transition(ChatLoopTransition::CancelCurrentLoop);
            let outcome = AgentRunOutcome {
                status: AgentRunStatus::Cancelled,
                turns: turn_count,
                duration: turn_start.elapsed(),
                role: None,
                model: client.model_name().to_string(),
            };
            let _ = finalize_main_loop(
                &outcome,
                &sink,
                &hook_ui,
                &hook_runner,
                &session_id,
                &turn_context,
                &task_store,
            )
            .await;
            loop_fsm.assert_state(ChatLoopState::Done, "cancel finalizes loop");
            break;
        }

        loop_fsm.transition(ChatLoopTransition::Compact);
        auto_compact(
            &sink,
            &hook_ui,
            &hook_runner,
            turn_count,
            &mut messages,
            &system_prompt_text,
            context_size,
            tool_schema_tokens,
            last_api_input_tokens,
            &memory_config,
            &cwd,
            &ctx.client,
        )
        .await;
        loop_fsm.transition(ChatLoopTransition::ResumeRunning);

        let gate = drain_and_apply_gate(
            GateKind::BeforeLlm,
            &mut pending_input,
            &queue,
            &input_events,
            &sink,
            &mut messages,
        )
        .await;
        match gate.decision {
            GateDecision::Proceed | GateDecision::ContinueNextTurn => {
                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
            }
            GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop => {
                loop_fsm.transition(chat_loop_transition_for_gate_exit(gate.decision));
                loop_fsm.assert_state(ChatLoopState::Done, "before-llm gate exits loop");
                sink.send_event(RuntimeStreamEvent::Cancelled {
                    context: turn_context.clone(),
                })
                .await;
                break;
            }
        }

        // Scan last assistant message for TaskCreate/TaskUpdate before building reminder
        task_reminder_state.update_from_messages(turn_count as u64, &messages);

        let messages_for_api: Vec<Message> = {
            let mut api_msgs = Vec::new();
            if !user_context.is_empty() {
                api_msgs.push(Message::user(format!(
                    "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
                )));
            }
            // Inject task reminder if conditions are met
            if let Some(reminder) = task_reminder_state
                .build_reminder(turn_count as u64, &task_store)
                .await
            {
                api_msgs.push(reminder);
            }
            api_msgs.extend(messages.iter().cloned());
            api_msgs
        };

        let mut handler = RuntimeStreamHandler::with_tool_identity(
            sink.clone(),
            tool_identity.clone(),
            turn_context.clone(),
        );
        log_llm_input(
            &messages_for_api,
            messages.len(),
            &system_blocks,
            &tool_schemas,
        );

        let api_start = std::time::Instant::now();
        let response = client
            .stream_message(
                &system_blocks,
                &messages_for_api,
                &tool_schemas,
                &mut handler,
                &cancel,
            )
            .await;
        let api_elapsed = api_start.elapsed().as_secs_f64();
        log::debug!(
            "turn api finished: session={}, turn={}, elapsed_secs={:.3}",
            session_id,
            turn_count,
            api_elapsed
        );
        match response {
            Ok(resp) => {
                last_api_input_tokens = resp.usage.input_tokens as u64;
                sink.send_event(RuntimeStreamEvent::Usage {
                    input: resp.usage.input_tokens,
                    output: resp.usage.output_tokens,
                    last_input: resp.usage.input_tokens,
                    elapsed_secs: api_elapsed,
                })
                .await;

                messages.push(resp.assistant_message.clone());
                sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                    .await;

                if stall_detector.record_text(&resp.assistant_message.text_content()) {
                    sink.send_event(RuntimeStreamEvent::SystemMessage(
                        "[agent loop stopped: LLM is producing repetitive output]".to_string(),
                    ))
                    .await;
                    loop_fsm.transition(ChatLoopTransition::TryStop);
                    let gate = drain_and_apply_gate(
                        GateKind::BeforeFinish,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut messages,
                    )
                    .await;
                    if gate.decision == GateDecision::ContinueNextTurn {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        continue;
                    }
                    loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                    loop_fsm.assert_state(ChatLoopState::Done, "stall stop finalizes loop");
                    break;
                }

                let tool_calls =
                    Agent::extract_tool_calls_with_ids(&resp.assistant_message, |provider_id| {
                        tool_identity.runtime_id_for_provider(provider_id)
                    });
                log_llm_output_and_tool_calls(
                    client.provider_name(),
                    &resp,
                    &tool_calls,
                    api_elapsed,
                );
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    loop_fsm.transition(ChatLoopTransition::TryStop);
                    let gate = drain_and_apply_gate(
                        GateKind::BeforeFinish,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut messages,
                    )
                    .await;
                    let before_finish_gate_continue =
                        gate.decision == GateDecision::ContinueNextTurn;
                    if before_finish_gate_continue {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        continue;
                    }
                    if should_run_turn_reflection(
                        &memory_config,
                        turn_count,
                        !tool_calls.is_empty(),
                        &resp.stop_reason,
                        before_finish_gate_continue,
                    ) {
                        if let Some(text) = run_reflection(
                            &memory_config,
                            turn_count,
                            &messages,
                            &cwd,
                            client,
                            &system_prompt_text,
                        )
                        .await
                        {
                            sink.send_event(RuntimeStreamEvent::SystemMessage(text))
                                .await;
                        }
                    }
                    let outcome = AgentRunOutcome {
                        status: AgentRunStatus::Completed,
                        turns: turn_count,
                        duration: turn_start.elapsed(),
                        role: None,
                        model: client.model_name().to_string(),
                    };
                    log_agent_outcome(&outcome, &session_id);
                    if let Some(outcome) = run_stop_hook_before_finish(
                        &outcome,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &session_id,
                    )
                    .await
                    {
                        loop_fsm.transition(ChatLoopTransition::StopBlocked);
                        messages.push(Message::system_generated_user(format!(
                            "<system-reminder>\n{outcome}\n</system-reminder>"
                        )));
                        sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                            .await;
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        loop_fsm
                            .assert_state(ChatLoopState::Running, "stop hook blocked resumes loop");
                        continue;
                    }
                    let gate = drain_and_apply_gate(
                        GateKind::BeforeFinish,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut messages,
                    )
                    .await;
                    if gate.decision == GateDecision::ContinueNextTurn {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        continue;
                    }
                    loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                    loop_fsm.assert_state(
                        ChatLoopState::Done,
                        "completed loop finalizes after stop hooks pass",
                    );
                    finish_completed_loop(&outcome, &sink, &turn_context, &task_store).await;
                    break;
                }
                {
                    loop_fsm.transition(ChatLoopTransition::AwaitTool);
                    let all_results = execute_tool_round(
                        &turn_context,
                        &tool_calls,
                        &registry,
                        allow_all,
                        &agent,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        max_agent_concurrency,
                        &interrupted,
                    )
                    .await;

                    // Build tool result message for API
                    messages.push(tool_results_for_api(all_results, &session_id));
                    // Sync after tool execution
                    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                        .await;
                    loop_fsm.transition(ChatLoopTransition::AwaitUser);
                    let gate = drain_and_apply_gate(
                        GateKind::AfterBlockingBoundary,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut messages,
                    )
                    .await;
                    if matches!(
                        gate.decision,
                        GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop
                    ) {
                        loop_fsm.transition(chat_loop_transition_for_gate_exit(gate.decision));
                        loop_fsm.assert_state(ChatLoopState::Done, "after-tool gate exits loop");
                        sink.send_event(RuntimeStreamEvent::Cancelled {
                            context: turn_context.clone(),
                        })
                        .await;
                        break;
                    }
                    loop_fsm.transition(ChatLoopTransition::ResumeRunning);

                    run_post_tool_batch(&sink, &hook_ui, &hook_runner, &agent.ctx, turn_count)
                        .await;
                }
            }
            Err(e) => {
                if is_user_cancelled_provider_error(&e)
                    // If user cancellation races with provider error reporting, classify
                    // generic abort/network errors as cancellation rather than API errors.
                    || cancel.is_cancelled()
                    || interrupted.load(Ordering::Relaxed)
                {
                    interrupted.store(false, Ordering::Relaxed);
                    messages.truncate(messages_at_start);
                    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                        .await;
                    sink.send_event(RuntimeStreamEvent::Cancelled {
                        context: turn_context.clone(),
                    })
                    .await;
                    loop_fsm.transition(ChatLoopTransition::CancelCurrentLoop);
                    let outcome = AgentRunOutcome {
                        status: AgentRunStatus::Cancelled,
                        turns: turn_count,
                        duration: turn_start.elapsed(),
                        role: None,
                        model: client.model_name().to_string(),
                    };
                    let _ = finalize_main_loop(
                        &outcome,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &session_id,
                        &turn_context,
                        &task_store,
                    )
                    .await;
                    loop_fsm.assert_state(ChatLoopState::Done, "api cancel finalizes loop");
                    break;
                }

                let error_msg = e.to_string();
                sink.send_event(RuntimeStreamEvent::Error(error_msg.clone()))
                    .await;
                let gate = drain_and_apply_gate(
                    GateKind::BeforeFinish,
                    &mut pending_input,
                    &queue,
                    &input_events,
                    &sink,
                    &mut messages,
                )
                .await;
                if gate.decision == GateDecision::ContinueNextTurn {
                    loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    continue;
                }
                loop_fsm.transition(ChatLoopTransition::TryStop);
                if let Some(outcome) = finalize_main_loop(
                    &AgentRunOutcome {
                        status: AgentRunStatus::ApiError(error_msg),
                        turns: turn_count,
                        duration: turn_start.elapsed(),
                        role: None,
                        model: client.model_name().to_string(),
                    },
                    &sink,
                    &hook_ui,
                    &hook_runner,
                    &session_id,
                    &turn_context,
                    &task_store,
                )
                .await
                {
                    messages.push(Message::system_generated_user(format!(
                        "<system-reminder>\n{outcome}\n</system-reminder>"
                    )));
                    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                        .await;
                    loop_fsm.transition(ChatLoopTransition::StopBlocked);
                    loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    loop_fsm.assert_state(
                        ChatLoopState::Running,
                        "api-error stop hook blocked resumes loop",
                    );
                    continue;
                }
                loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                loop_fsm.assert_state(
                    ChatLoopState::Done,
                    "api-error finalizes after stop hooks pass",
                );
                break;
            }
        }
    }
}

fn chat_loop_transition_for_gate_exit(decision: GateDecision) -> ChatLoopTransition {
    match decision {
        GateDecision::AbortCurrentLoop => ChatLoopTransition::AbortCurrentLoop,
        GateDecision::CancelCurrentLoop => ChatLoopTransition::CancelCurrentLoop,
        GateDecision::Proceed | GateDecision::ContinueNextTurn => {
            unreachable!("only abort/cancel decisions should exit the chat loop")
        }
    }
}

fn is_user_cancelled_provider_error(error: &provider::api::LlmError) -> bool {
    error.is_cancelled()
}

async fn drain_and_apply_gate<Q, I, S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    queue: &Q,
    input_events: &I,
    sink: &S,
    messages: &mut Vec<Message>,
) -> crate::business::chat::GateOutcome
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    crate::business::chat::looping::drain_sources(buffer, queue, input_events).await;
    crate::business::chat::looping::apply_gate(kind, buffer, sink, messages).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hook::api::HookRunner;
    use provider::api::{LlmProvider, StreamHandler};
    use provider::api::{StopReason, StreamResponse, SystemBlock, Usage};
    use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
    use share::message::{MessageSource, Role};
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn provider_cancelled_error_maps_to_cancelled_outcome() {
        let error = provider::api::LlmError::Cancelled;
        assert!(is_user_cancelled_provider_error(&error));
    }

    #[derive(Clone)]
    struct SequenceQueueDrainPort {
        responses: Arc<Mutex<VecDeque<Option<Vec<String>>>>>,
    }

    impl SequenceQueueDrainPort {
        fn new(responses: Vec<Option<Vec<String>>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }
    }

    impl QueueDrainPort for SequenceQueueDrainPort {
        fn drain_queued_input<'a>(&'a self) -> crate::business::chat::looping::QueueFuture<'a> {
            Box::pin(async move { self.responses.lock().unwrap().pop_front().flatten() })
        }
    }

    #[derive(Clone, Default)]
    struct EmptyInputEvents;

    impl InputEventDrainPort for EmptyInputEvents {
        fn drain_input_events<'a>(
            &'a self,
        ) -> crate::business::chat::looping::InputEventFuture<'a> {
            Box::pin(async { Vec::new() })
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSink {
        events: Arc<Mutex<Vec<String>>>,
        messages_syncs: Arc<Mutex<Vec<Vec<Message>>>>,
    }

    impl ChatEventSink for RecordingSink {
        fn send_event<'a>(
            &'a self,
            event: RuntimeStreamEvent,
        ) -> crate::business::chat::looping::EventFuture<'a> {
            Box::pin(async move {
                self.record(event);
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.record(event);
        }
    }

    impl RecordingSink {
        fn record(&self, event: RuntimeStreamEvent) {
            let name = match event {
                RuntimeStreamEvent::MessagesSync(messages) => {
                    self.messages_syncs.lock().unwrap().push(messages.clone());
                    format!(
                        "MessagesSync:{}",
                        messages
                            .last()
                            .map(|message| message.text_content())
                            .unwrap_or_default()
                    )
                }
                RuntimeStreamEvent::DoneWithDuration { .. } => "DoneWithDuration".to_string(),
                RuntimeStreamEvent::HookEvent(event) => {
                    format!("HookEvent:{}:{:?}", event.hook_name, event.status)
                }
                RuntimeStreamEvent::TurnChanged(turn) => format!("TurnChanged:{turn}"),
                RuntimeStreamEvent::Usage { .. } => "Usage".to_string(),
                RuntimeStreamEvent::Text { text, .. } => format!("Text:{text}"),
                RuntimeStreamEvent::Done { .. } => "Done".to_string(),
                RuntimeStreamEvent::SystemMessage(message) => format!("SystemMessage:{message}"),
                RuntimeStreamEvent::Error(message) => format!("Error:{message}"),
                RuntimeStreamEvent::Cancelled { .. } => "Cancelled".to_string(),
                RuntimeStreamEvent::Thinking { .. } => "Thinking".to_string(),
                RuntimeStreamEvent::BlockComplete { .. } => "BlockComplete".to_string(),
                RuntimeStreamEvent::ToolCallStart { .. } => "ToolCallStart".to_string(),
                RuntimeStreamEvent::ToolCallUpdate { .. } => "ToolCallUpdate".to_string(),
                RuntimeStreamEvent::ToolResult { .. } => "ToolResult".to_string(),
                RuntimeStreamEvent::LiveTps(_) => "LiveTps".to_string(),
                RuntimeStreamEvent::AskUser { .. } => "AskUser".to_string(),
                RuntimeStreamEvent::AgentProgress { .. } => "AgentProgress".to_string(),
                RuntimeStreamEvent::WorkingDirectoryChanged { .. } => {
                    "WorkingDirectoryChanged".to_string()
                }
                RuntimeStreamEvent::TasksChanged => "TasksChanged".to_string(),
            };
            self.events.lock().unwrap().push(name);
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }

        fn synced_messages(&self) -> Vec<Vec<Message>> {
            self.messages_syncs.lock().unwrap().clone()
        }
    }

    struct TwoTurnProvider;

    #[async_trait]
    impl LlmProvider for TwoTurnProvider {
        async fn stream_message(
            &self,
            _system: &[SystemBlock],
            messages: &[Message],
            _tool_schemas: &[serde_json::Value],
            handler: &mut dyn StreamHandler,
            _cancel: &CancellationToken,
        ) -> Result<StreamResponse, provider::LlmError> {
            let text = if messages
                .iter()
                .any(|message| message.text_content() == "stop-hook input")
            {
                "handled queued input"
            } else {
                "initial final response"
            };
            handler.on_text(text);
            Ok(StreamResponse {
                assistant_message: Message {
                    role: share::message::Role::Assistant,
                    content: vec![share::message::ContentBlock::Text {
                        text: text.to_string(),
                    }],
                    metadata: None,
                },
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::EndTurn,
            })
        }

        fn model_name(&self) -> &str {
            "test-model"
        }

        fn provider_name(&self) -> &str {
            "test-provider"
        }

        fn set_reasoning(&self, _enabled: bool) {}

        fn is_reasoning(&self) -> bool {
            false
        }
    }

    struct SequenceProvider {
        responses: Arc<Mutex<VecDeque<String>>>,
    }

    impl SequenceProvider {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(
                    responses.into_iter().map(str::to_string).collect(),
                )),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for SequenceProvider {
        async fn stream_message(
            &self,
            _system: &[SystemBlock],
            _messages: &[Message],
            _tool_schemas: &[serde_json::Value],
            handler: &mut dyn StreamHandler,
            _cancel: &CancellationToken,
        ) -> Result<StreamResponse, provider::LlmError> {
            let text = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| "fallback final response".to_string());
            handler.on_text(&text);
            Ok(StreamResponse {
                assistant_message: Message {
                    role: share::message::Role::Assistant,
                    content: vec![share::message::ContentBlock::Text { text }],
                    metadata: None,
                },
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::EndTurn,
            })
        }

        fn model_name(&self) -> &str {
            "test-model"
        }

        fn provider_name(&self) -> &str {
            "test-provider"
        }

        fn set_reasoning(&self, _enabled: bool) {}

        fn is_reasoning(&self) -> bool {
            false
        }
    }

    fn test_hook_runner() -> HookRunner {
        let mut events = HashMap::new();
        events.insert(
            HookEvent::Stop,
            vec![HookEntry {
                matcher: String::new(),
                command: "true".to_string(),
                timeout: 5,
            }],
        );
        HookRunner::new(HooksConfig { events }, ".".to_string())
    }

    fn blocking_then_success_hook_runner(flag_path: &std::path::Path) -> HookRunner {
        // 用 nanos 时间戳生成唯一 flag 路径，避免与并行 cargo test 共享
        // target/stop-hook-once.flag 时的 race condition。
        let flag_path_str = flag_path.to_string_lossy().to_string();
        let mut events = HashMap::new();
        events.insert(
            HookEvent::Stop,
            vec![HookEntry {
                matcher: String::new(),
                command: format!(
                    "python3 -c 'import pathlib, sys; \
                     p=pathlib.Path(\"{flag_path}\"); \
                     sys.exit(0 if p.exists() else (p.parent.mkdir(parents=True, exist_ok=True), \
                     p.write_text(\"blocked\"), print(\"fix before stopping\"), 2)[3])'",
                    flag_path = flag_path_str,
                ),
                timeout: 5,
            }],
        );
        HookRunner::new(HooksConfig { events }, ".".to_string())
    }

    #[tokio::test]
    async fn test_process_chat_loop_stop_hook_blocked_continues_until_success() {
        // 每次测试生成独立 flag 路径，避免 cargo test 并行 race。
        let flag_path = std::env::temp_dir().join(format!(
            "aemeath_stop_hook_once_{}.flag",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&flag_path);
        let sink = RecordingSink::default();

        process_chat_loop(ChatLoopContext {
            sink: sink.clone(),
            queue: SequenceQueueDrainPort::new(vec![None, None, None, None]),
            input_events: EmptyInputEvents,
            client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
                SequenceProvider::new(vec!["first attempted final", "after hook feedback"]),
            ))),
            registry: Arc::new(ToolRegistry::new()),
            system_blocks: Vec::new(),
            system_prompt_text: String::new(),
            user_context: String::new(),
            messages: vec![Message::user("hello")],
            context_size: 200_000,
            cwd: std::env::current_dir().unwrap(),
            workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
            session_id: "test-stop-hook-blocked".to_string(),
            read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            session_reminders: Arc::new(
                std::sync::Mutex::new(share::tool::SessionReminders::new()),
            ),
            agent_runner: None,
            allow_all: false,
            interrupted: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
            task_store: Arc::new(storage::api::TaskStore::new()),
            max_tool_concurrency: 1,
            max_agent_concurrency: 1,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            hook_runner: blocking_then_success_hook_runner(&flag_path),
            memory_config: share::config::MemoryConfig::default(),
        })
        .await;
        let _ = std::fs::remove_file(&flag_path);

        let events = sink.events();
        let feedback_sync = events
            .iter()
            .position(|event| {
                event.starts_with("MessagesSync:")
                    && event.contains("你 MUST 先满足下面 Stop hook 的要求")
            })
            .expect("blocked Stop hook feedback should be synced into messages");
        let hook_notice = events
            .iter()
            .position(|event| event == "HookEvent:Stop:Blocked")
            .expect("blocked Stop hook should emit typed hook event");
        let second_text = events
            .iter()
            .position(|event| event == "Text:after hook feedback")
            .expect("blocked Stop hook should continue to another LLM turn");
        let done = events
            .iter()
            .position(|event| event == "DoneWithDuration")
            .expect("loop should finish after Stop hook succeeds");

        assert!(hook_notice < feedback_sync);
        assert!(feedback_sync < second_text);
        assert!(second_text < done);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn test_stop_hook_feedback_message_is_marked_system_generated() {
        let flag_path = std::env::temp_dir().join(format!(
            "aemeath_stop_hook_metadata_{}.flag",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&flag_path);
        let sink = RecordingSink::default();

        process_chat_loop(ChatLoopContext {
            sink: sink.clone(),
            queue: SequenceQueueDrainPort::new(vec![None, None, None, None]),
            input_events: EmptyInputEvents,
            client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
                SequenceProvider::new(vec!["first attempted final", "after hook feedback"]),
            ))),
            registry: Arc::new(ToolRegistry::new()),
            system_blocks: Vec::new(),
            system_prompt_text: String::new(),
            user_context: String::new(),
            messages: vec![Message::user("hello")],
            context_size: 200_000,
            cwd: std::env::current_dir().unwrap(),
            workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
            session_id: "test-stop-hook-metadata".to_string(),
            read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            session_reminders: Arc::new(
                std::sync::Mutex::new(share::tool::SessionReminders::new()),
            ),
            agent_runner: None,
            allow_all: false,
            interrupted: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
            task_store: Arc::new(storage::api::TaskStore::new()),
            max_tool_concurrency: 1,
            max_agent_concurrency: 1,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            hook_runner: blocking_then_success_hook_runner(&flag_path),
            memory_config: share::config::MemoryConfig::default(),
        })
        .await;
        let _ = std::fs::remove_file(&flag_path);

        let feedback = sink
            .synced_messages()
            .into_iter()
            .flatten()
            .find(|message| {
                message
                    .text_content()
                    .contains("你 MUST 先满足下面 Stop hook 的要求")
            })
            .expect("blocked Stop hook feedback should be synced into messages");

        assert_eq!(feedback.role, Role::User);
        assert_eq!(feedback.source(), MessageSource::SystemGenerated);
    }

    #[tokio::test]
    async fn test_process_chat_loop_uses_workspace_working_root_for_stop_hook_env() {
        let sink = RecordingSink::default();
        let path_base = tempfile::tempdir().unwrap();
        let working_root = tempfile::tempdir().unwrap();
        let marker = path_base.path().join("stop-hook-env.txt");
        let marker_path = marker.display().to_string();
        let workspace_dto = crate::business::session::PersistedWorkspaceContext {
            path_base: path_base.path().display().to_string(),
            working_root: working_root.path().display().to_string(),
            context_stack: vec![crate::business::session::PersistedWorkspaceFrame {
                path_base: path_base.path().display().to_string(),
                working_root: path_base.path().display().to_string(),
            }],
        };
        let workspace = project::api::WorkspaceService::new(path_base.path().to_path_buf());
        project::api::WorkspacePersist::restore(workspace.as_ref(), &workspace_dto)
            .expect("restore workspace dto");
        let mut events = HashMap::new();
        events.insert(
            HookEvent::Stop,
            vec![HookEntry {
                matcher: String::new(),
                command: format!(
                    "printf '%s|%s|%s' \"$AEMEATH_PROJECT_DIR\" \"$CLAUDE_PROJECT_DIR\" \"$PWD\" > \"{}\"",
                    marker_path
                ),
                timeout: 5,
            }],
        );

        process_chat_loop(ChatLoopContext {
            sink: sink.clone(),
            queue: SequenceQueueDrainPort::new(vec![None, None]),
            input_events: EmptyInputEvents,
            client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
                SequenceProvider::new(vec!["final response"]),
            ))),
            registry: Arc::new(ToolRegistry::new()),
            system_blocks: Vec::new(),
            system_prompt_text: String::new(),
            user_context: String::new(),
            messages: vec![Message::user("hello")],
            context_size: 200_000,
            cwd: path_base.path().to_path_buf(),
            workspace,
            session_id: "test-worktree-stop-hook-env".to_string(),
            read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            session_reminders: Arc::new(
                std::sync::Mutex::new(share::tool::SessionReminders::new()),
            ),
            agent_runner: None,
            allow_all: false,
            interrupted: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
            task_store: Arc::new(storage::api::TaskStore::new()),
            max_tool_concurrency: 1,
            max_agent_concurrency: 1,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            hook_runner: HookRunner::new(
                HooksConfig { events },
                path_base.path().display().to_string(),
            ),
            memory_config: share::config::MemoryConfig::default(),
        })
        .await;

        assert!(sink
            .events()
            .iter()
            .any(|event| event == "HookEvent:Stop:Succeeded"));
        let output = std::fs::read_to_string(marker).unwrap();
        let parts: Vec<&str> = output.split('|').collect();
        assert_eq!(parts.len(), 3);
        let expected = working_root.path().canonicalize().unwrap();
        for part in parts {
            assert_eq!(std::fs::canonicalize(part).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn test_process_chat_loop_drains_input_after_stop_hook_before_done() {
        let sink = RecordingSink::default();
        let queue = SequenceQueueDrainPort::new(vec![
            None,
            Some(vec!["stop-hook input".to_string()]),
            None,
            None,
        ]);

        process_chat_loop(ChatLoopContext {
            sink: sink.clone(),
            queue,
            input_events: EmptyInputEvents,
            client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
                TwoTurnProvider,
            ))),
            registry: Arc::new(ToolRegistry::new()),
            system_blocks: Vec::new(),
            system_prompt_text: String::new(),
            user_context: String::new(),
            messages: vec![Message::user("hello")],
            context_size: 200_000,
            cwd: std::env::current_dir().unwrap(),
            workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
            session_id: "test-session".to_string(),
            read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            session_reminders: Arc::new(
                std::sync::Mutex::new(share::tool::SessionReminders::new()),
            ),
            agent_runner: None,
            allow_all: false,
            interrupted: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
            task_store: Arc::new(storage::api::TaskStore::new()),
            max_tool_concurrency: 1,
            max_agent_concurrency: 1,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            hook_runner: test_hook_runner(),
            memory_config: share::config::MemoryConfig::default(),
        })
        .await;

        let events = sink.events();
        let queued_sync = events
            .iter()
            .position(|event| event == "MessagesSync:stop-hook input")
            .expect("queued input should be synced after Stop hook");
        let done = events
            .iter()
            .position(|event| event == "DoneWithDuration")
            .expect("loop should eventually finish");
        let handled_text = events
            .iter()
            .position(|event| event == "Text:handled queued input")
            .expect("queued input should trigger another LLM turn");

        assert!(queued_sync < handled_text);
        assert!(handled_text < done);
    }
}
