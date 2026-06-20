use crate::business::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::business::agent::Agent;
use crate::business::chat::looping::apply_gate;
use crate::business::chat::looping::compact::auto_compact;
use crate::business::chat::looping::finalize::{
    finalize_main_loop, finish_completed_loop, run_stop_hook_before_finish,
    stop_hook_block_limit_reached,
};
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::business::chat::looping::loop_helpers::{
    chat_loop_transition_for_gate_exit, drain_and_apply_gate, is_user_cancelled_provider_error,
};
use crate::business::chat::looping::loop_phases::{
    build_api_messages, handle_turn_boundary_config,
};
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
use crate::LOG_TARGET;
use provider::api::StopReason;
use sdk::ids::{ChatId, ChatTurnId};
use share::message::Message;
use std::path::PathBuf;
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
    pub cancel: CancellationToken,
    pub task_store: Arc<storage::api::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: hook::api::HookRunner,
    pub memory_config: share::config::MemoryConfig,
    pub language: String,
    /// Compact 时冻结的旧链（保留在 session 文件中供审计，resume 不加载）。
    pub frozen_chats: Arc<std::sync::Mutex<Vec<crate::business::session::ChatSegment>>>,
    /// 活跃链的 compact summary（走 system 通道注入）。
    pub active_summary: Arc<std::sync::Mutex<Option<String>>>,
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
        cancel,
        task_store,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        hook_runner,
        memory_config,
        language,
        frozen_chats,
        active_summary: active_summary_arc,
    } = ctx;
    let hook_ui = HookUi::new(sink.clone());

    // workspace service 跨 chat 轮次持有：恢复 session 时已 restore 到正确位置，
    // 这里直接读取当前 root 作为 hook/日志的工作目录基准（忽略 seed cwd）。
    let cwd = project::api::WorkspaceRead::current_root(workspace.as_ref());
    let in_worktree = project::api::WorkspaceRead::in_worktree(workspace.as_ref());
    hook_runner.set_project_context(cwd.display().to_string(), in_worktree);
    log::info!(target: LOG_TARGET,
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
            registry: Some(registry.clone() as std::sync::Arc<dyn tools::api::ToolListProvider>),
        },
    };

    let messages_at_start = messages.len();
    let mut active_summary: Option<String> = None;
    let mut last_api_input_tokens: u64 = 0;
    let mut last_api_output_tokens: u64 = 0;
    let mut cached_tokens: Option<u64> = None;
    let mut reasoning_tokens: Option<u64> = None;
    let turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;
    let mut task_reminder_state = TaskReminderState::new();
    let mut stall_detector = StallDetector::new();
    let mut stop_hook_block_count: usize = 0;
    let mut pending_input = PendingInputBuffer::default();
    let mut loop_fsm = ChatLoopFsm::default();
    let tool_identity = crate::business::chat::looping::tool_identity::ToolIdentityRegistry::new();
    let chat_id = ChatId::new_v7();
    // 将 chat_id 同步到日志 context，影响 tool/audit/hook 等共享 sink 的 chat 字段
    logging::context::set_current_chat_id(chat_id.to_string());
    // 初始化配置变更快照注册表（turn 边界轮询用）
    let mut config_snapshot =
        crate::business::chat::looping::config_reload::init_snapshot_registry(&cwd);
    loop {
        turn_count += 1;
        let turn_id = ChatTurnId::new_v7();
        let turn_context = RuntimeTurnContext::new(chat_id.clone(), turn_id);
        loop_fsm.transition(ChatLoopTransition::StartTurn);
        sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
            .await;

        // ── turn 边界：检测配置/指令/guidance 文件变更 ──
        handle_turn_boundary_config(
            &mut config_snapshot,
            turn_count,
            &sink,
            &mut messages,
            &language,
        )
        .await;

        // Refresh tool schemas each turn so dynamically registered MCP tools
        // are visible to the LLM once the background connector finishes.
        let tool_schemas = registry.schemas();
        let tool_schema_tokens =
            crate::business::compact::estimate_tool_schemas_tokens(&tool_schemas);

        if cancel.is_cancelled() {
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
                &language,
            )
            .await;
            loop_fsm.assert_state(ChatLoopState::Done, "cancel finalizes loop");
            break;
        }

        loop_fsm.transition(ChatLoopTransition::Compact);
        // compact：发生时替换 messages 为 recent tail，summary 走 system。
        // resume 保护 + 产生时定型原则下，messages 产生后只在 compact 时被替换。
        if let Some(outcome) = auto_compact(
            &sink,
            &hook_ui,
            &hook_runner,
            turn_count,
            &messages,
            &system_prompt_text,
            context_size,
            tool_schema_tokens,
            last_api_input_tokens,
            last_api_output_tokens,
            cached_tokens,
            reasoning_tokens,
            &memory_config,
            &cwd,
            &ctx.client,
        )
        .await
        {
            // 1. 冻结旧链（compact 前的完整 messages）到 frozen_chats，
            //    保证 session 真相源完整（resume 不加载，但落盘保留）。
            let old_segment = {
                use crate::business::session::ChatSegment;
                let mut seg = ChatSegment::normal(None);
                seg.messages = std::mem::take(&mut messages);
                seg
            };
            if let Ok(mut guard) = frozen_chats.lock() {
                guard.push(old_segment);
            }

            // 2. 替换为 recent tail
            messages = outcome.messages;

            // 3. 设 summary（走 system 通道）
            active_summary = Some(outcome.summary);
            if let Ok(mut guard) = active_summary_arc.lock() {
                *guard = active_summary.clone();
            }
            sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                .await;
        }
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

        let messages_for_api: Vec<Message> = build_api_messages(
            &user_context,
            &language,
            &mut task_reminder_state,
            turn_count as u64,
            &task_store,
            &messages,
        )
        .await;

        let mut handler = RuntimeStreamHandler::with_tool_identity(
            sink.clone(),
            tool_identity.clone(),
            turn_context.clone(),
        );
        // 设置日志 context（每次 LLM 调用前）
        logging::context::set_current_model(client.model_name().to_string());
        logging::context::set_current_provider(client.provider_name().to_string());
        logging::context::set_current_role("default".to_string());
        let request_id = uuid::Uuid::now_v7().to_string();
        logging::context::set_current_request_id(request_id);

        // summary 注入 system_blocks（compact 后的摘要走 system 通道）
        let mut effective_system_blocks = system_blocks.clone();
        if let Some(ref summary) = active_summary.clone() {
            effective_system_blocks.push(provider::api::SystemBlock {
                block_type: "text".to_string(),
                text: format!("<compact-summary>\n{summary}\n</compact-summary>"),
                cache_control: None,
            });
        }

        log_llm_input(
            &messages_for_api,
            messages.len(),
            &effective_system_blocks,
            &tool_schemas,
        );

        let api_start = std::time::Instant::now();
        let response = client
            .stream_message(
                &effective_system_blocks,
                &messages_for_api,
                &tool_schemas,
                &mut handler,
                &cancel,
            )
            .await;
        let api_elapsed = api_start.elapsed().as_secs_f64();
        log::debug!(target: LOG_TARGET,
            "turn api finished: session={}, turn={}, elapsed_secs={:.3}",
            session_id,
            turn_count,
            api_elapsed
        );
        match response {
            Ok(resp) => {
                last_api_input_tokens = resp.usage.input_tokens as u64;
                last_api_output_tokens = resp.usage.output_tokens as u64;
                cached_tokens = resp.usage.cached_tokens.map(|v| v as u64);
                let cache_creation = resp.usage.cache_creation_tokens.map(|v| v as u64);
                reasoning_tokens = resp.usage.reasoning_tokens.map(|v| v as u64);

                // 计算 context window 使用情况
                let cached = cached_tokens.unwrap_or(0);
                let cache_write = cache_creation.unwrap_or(0);
                let reasoning = reasoning_tokens.unwrap_or(0);
                let total_tokens = last_api_input_tokens + last_api_output_tokens + reasoning;
                let effective_window =
                    crate::business::compact::effective_context_window(context_size, 8192) as u64;
                let threshold =
                    crate::business::compact::autocompact_threshold(context_size, 8192) as u64;
                let pct = total_tokens * 100 / effective_window.max(1);

                log::info!(target: LOG_TARGET,
                    "turn usage: session={}, turn={}, input={}, output={}, cache_write={}, cached={}, reasoning={}, total={}, context_size={}, effective_window={}, threshold={}, usage_pct={}%",
                    session_id,
                    turn_count,
                    last_api_input_tokens,
                    last_api_output_tokens,
                    cache_write,
                    cached,
                    reasoning,
                    total_tokens,
                    context_size,
                    effective_window,
                    threshold,
                    pct
                );

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
                    // #372: stall 终止前也须经 Stop hook 门禁；阻断则注入反馈并重试
                    let stall_outcome = AgentRunOutcome {
                        status: AgentRunStatus::Completed,
                        turns: turn_count,
                        duration: turn_start.elapsed(),
                        role: None,
                        model: client.model_name().to_string(),
                    };
                    if let Some(feedback) = run_stop_hook_before_finish(
                        &stall_outcome,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &session_id,
                        &language,
                    )
                    .await
                    {
                        stop_hook_block_count += 1;
                        if stop_hook_block_limit_reached(
                            stop_hook_block_count,
                            &sink,
                            &mut loop_fsm,
                        )
                        .await
                        {
                            break;
                        }
                        loop_fsm.transition(ChatLoopTransition::StopBlocked);
                        messages.push(Message::system_generated_user(format!(
                            "<system-reminder>\n{feedback}\n</system-reminder>"
                        )));
                        sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                            .await;
                        stall_detector = StallDetector::new();
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
                        &language,
                    )
                    .await
                    {
                        stop_hook_block_count += 1;
                        if stop_hook_block_limit_reached(
                            stop_hook_block_count,
                            &sink,
                            &mut loop_fsm,
                        )
                        .await
                        {
                            break;
                        }
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
                    // 回合完成、stop hook 放行：发出 Done，但不退出常驻 loop。
                    // 进入空闲态阻塞等待下一条输入；通道关闭才 shutdown 退出。
                    finish_completed_loop(&outcome, &sink, &turn_context, &task_store).await;
                    loop_fsm.transition(ChatLoopTransition::Idle);
                    loop_fsm.assert_state(
                        ChatLoopState::Idle,
                        "completed loop idles after stop hooks pass",
                    );
                    let mut shutdown = false;
                    loop {
                        match await_idle_input(&input_events, &mut pending_input).await {
                            IdleResult::Resumed => {
                                // 处理空闲期收到的事件：append 用户消息 / 识别 Cancel/clear / 命令。
                                let gate = apply_gate(
                                    GateKind::BeforeLlm,
                                    &mut pending_input,
                                    &sink,
                                    &mut messages,
                                )
                                .await;
                                // 仅当确有新用户消息 append 时才退出空闲跑回合。
                                // 单独的 ControlCommand（如 /save、/model、/provider）append 0 条
                                // 用户消息、decision 仍为 Proceed —— 此时若退出 idle 会在陈旧历史上
                                // 跑一个无新输入的空回合，违反 ControlCommand「永不作为 user message
                                // 发给 LLM」契约。Cancel/clear（Abort/Cancel 决策）同样 append 0 条，
                                // 被本条件一并涵盖 → 保持空闲继续等下一条。
                                // （命令副作用的实际应用属 Task 3 / #391 范畴，此处只负责不跑空回合。）
                                if gate.appended_user_messages > 0 {
                                    // 收到用户消息（已 append 进 messages）：恢复运行。
                                    break;
                                }
                                // 0 append（命令 / 取消 / 空）→ 留在空闲，继续等下一条输入。
                                continue;
                            }
                            IdleResult::Shutdown => {
                                shutdown = true;
                                break;
                            }
                        }
                    }
                    if shutdown {
                        loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                        loop_fsm.assert_state(
                            ChatLoopState::Done,
                            "idle loop shuts down on channel close",
                        );
                        break;
                    }
                    loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    continue;
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
                        &cancel,
                        &language,
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
                {
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
                        &language,
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
                    &language,
                )
                .await
                {
                    stop_hook_block_count += 1;
                    if stop_hook_block_limit_reached(stop_hook_block_count, &sink, &mut loop_fsm)
                        .await
                    {
                        break;
                    }
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

/// 空闲等待结果：收到下一条输入（恢复运行）或通道关闭（shutdown）。
enum IdleResult {
    Resumed,
    Shutdown,
}

/// 回合完成后阻塞等待下一条输入：
/// - 收到事件 → push 进 `pending` 缓冲，返回 `Resumed`（由调用方经 gate 处理）。
/// - `None`（通道关闭）→ 返回 `Shutdown`，调用方退出常驻 loop。
async fn await_idle_input<I: InputEventDrainPort>(
    input_events: &I,
    pending: &mut PendingInputBuffer,
) -> IdleResult {
    match input_events.recv_next_input().await {
        Some(event) => {
            pending.push(event);
            IdleResult::Resumed
        }
        None => IdleResult::Shutdown,
    }
}
