use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use sdk::ids::{ChatId, ChatTurnId};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::chat::looping::idle_lifecycle::{
    execute_set_thinking, idle_until_resume_or_shutdown, IdleResult,
};
use crate::application::chat::looping::input_gate::apply_gate;
use crate::application::chat::looping::loop_phases::handle_turn_boundary_config;
use crate::application::chat::looping::task_reminder::TaskReminderState;
use crate::application::chat::looping::{
    ChatEventSink, GateKind, InputEventDrainPort, PendingCommand, PendingInputBuffer,
    QueueDrainPort, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::loop_engine::run_loop;
use crate::domain::agent_run::{Run, RunSpec};
use workflow::api::ReasoningSignal;

use super::loop_context::ChatLoopContext;

#[path = "main_run_port.rs"]
pub(crate) mod main_run_port;
use main_run_port::MainRunPort;

/// Session actor for Main chat. The session itself only idles, accepts one real user input,
/// creates one fresh `Run`, drives it to a terminal state through the shared engine, then idles
/// again. `Run` is the only production state machine inside an active turn.
pub async fn process_chat_loop<S, Q, I>(ctx: ChatLoopContext<S, Q, I>)
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    let session_id_for_scope = ctx.session_id.clone();
    let chat_id = ChatId::new_v7();
    logging::within(
        logging::LogContextPatch {
            session_id: logging::FieldPatch::Set(session_id_for_scope),
            chat_id: logging::FieldPatch::Set(chat_id.to_string()),
            ..logging::LogContextPatch::default()
        },
        async move {
            let ChatLoopContext {
                sink,
                queue,
                input_events,
                binding,
                tool_catalog,
                tool_execution,
                tool_context_binding,
                system_blocks,
                system_prompt_text,
                initial_git_context,
                user_context,
                initial_messages,
                mut context_size,
                workspace,
                wiring,
                mut session_id,
                read_files,
                session_reminders,
                agent_runner,
                tool_result_materializer,
                policy,
                active_run,
                task_access,
                max_tool_concurrency,
                agent_semaphore,
                hook_runner,
                memory_config,
                memory: _,
                reflection_history,
                language,
                reasoning,
                build_switched_client,
                list_reflection_history,
                list_models,
                list_reminders,
                list_sessions,
            } = ctx;
            let mut binding = binding;
            let mut messages = initial_messages;
            let mut initial_git_context = (!initial_git_context.is_empty())
                .then_some(Message::system_generated_user(initial_git_context));
            // Interval and PreCompact share this single session-scoped slot.
            let reflection_tasks =
                crate::application::reflection::ReflectionTaskAdapter::production(
                    std::time::Duration::from_secs(120),
                );
            let mut cwd = workspace.read().current_workspace_root();
            let mut last_total_tokens = None;
            let mut turn_count = 0;
            let mut pending_input = PendingInputBuffer::default();
            let mut deferred_user_inputs = VecDeque::new();
            let mut task_reminder_state = TaskReminderState::new();
            let tool_identity =
                crate::application::tool_coordination::identity::ToolIdentityRegistry::new();
            let mut config_snapshot =
                crate::application::chat::looping::config_reload::init_snapshot_registry(&cwd);
            macro_rules! handle_pending_command {
        ($cmd:expr) => {
            match $cmd {
                PendingCommand::Compact => {
                    let bound = match wiring.bind_main_run().await {
                        Ok(bound) => bound,
                        Err(error) => {
                            sink.send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("无法绑定当前 Session：{error}"),
                                is_error: true,
                            }).await;
                            continue;
                        }
                    };
                    let coordinator = crate::application::context_coordination::ContextCoordinator::new(bound.context());
                    let request = crate::ports::ManualCompactRequest {
                        session_id: crate::ports::SessionId::new(bound.session().id.clone()),
                        run_id: sdk::RunId::new(uuid::Uuid::now_v7().to_string()),
                        system_prompt: crate::ports::SystemPromptSpec::new(system_prompt_text.clone()),
                        context_size,
                    };
                    match coordinator.manual_compact(&request).await {
                        Ok(crate::ports::CompactOutcome::Committed(result)) => {
                            messages = result.recent_messages.clone();
                            sink.send_event(RuntimeStreamEvent::CompactFinished {
                                messages: result.recent_messages,
                            }).await;
                        }
                        Ok(crate::ports::CompactOutcome::Skipped(_)) => {
                            sink.send_event(RuntimeStreamEvent::SystemMessage(
                                "Not enough messages to compact.".to_string(),
                            )).await;
                        }
                        Err(error) => {
                            sink.send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("Session compact 失败：{error}"),
                                is_error: true,
                            }).await;
                        }
                    }
                    continue;
                }
                PendingCommand::SwitchModel { selection } => {
                    match (build_switched_client)(&selection).await {
                        Ok((new_binding, result)) => {
                            reasoning.reset_default_level(new_binding.requested_reasoning);
                            binding = Arc::new(new_binding);
                            context_size = result.context_window;
                            let _ = sink
                                .send_event(RuntimeStreamEvent::ModelSwitched { result })
                                .await;
                        }
                        Err(msg) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: msg,
                                    is_error: true,
                                })
                                .await;
                        }
                    }
                    continue;
                }
                PendingCommand::SetThinking { desired } => {
                    execute_set_thinking(reasoning.as_ref(), &sink, desired).await;
                    continue;
                }
                PendingCommand::InitProject { force } => {
                    let cwd_str = cwd.display().to_string();
                    let (text, is_error) = super::idle_commands::execute_init(&cwd_str, force);
                    let _ = sink
                        .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                        .await;
                    continue;
                }
                PendingCommand::ManageSession { args } => {
                    let trimmed = args.trim();
                    if trimmed.is_empty() || trimmed == "list" {
                        match list_sessions().await {
                            Ok(sessions) => {
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::SessionList { sessions })
                                    .await;
                            }
                            Err(e) => {
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text: format!("List sessions failed: {e}"),
                                        is_error: true,
                                    })
                                    .await;
                            }
                        }
                    } else {
                        let port = wiring.session_management();
                        let args = args.clone();
                        let active_session_id = session_id.clone();
                        let result = wiring
                            .with_shared(async move {
                                super::idle_commands::execute_session(
                                    &args,
                                    &active_session_id,
                                    port.as_ref(),
                                )
                                .await
                            })
                            .await;
                        let (text, is_error) = result.unwrap_or_else(|_| {
                            ("Session is being switched, please retry.".to_string(), true)
                        });
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text,
                                is_error,
                            })
                            .await;
                    }
                    continue;
                }
                PendingCommand::ManageMemory { args } => {
                    let wiring_for_memory = wiring.clone();
                    let config = memory_config.clone();
                    let result = wiring
                        .with_shared(async move {
                            let memory = wiring_for_memory.committed_memory();
                            super::idle_commands::execute_memory(&args, memory.as_ref(), &config)
                                .await
                        })
                        .await;
                    let (text, is_error) = result.unwrap_or_else(|_| {
                        ("Session is being switched, please retry.".to_string(), true)
                    });
                    let _ = sink
                        .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                        .await;
                    continue;
                }
                PendingCommand::ResumeSession { id } => {
                    match crate::application::client::resume_helper::resume_session_to_backing(
                        &id,
                        &wiring,
                    )
                    .await
                    {
                        Ok(projection) => {
                            session_id = projection.session_id.clone();
                            messages = projection.messages.clone();
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumed {
                                    messages: projection.messages,
                                    session_id: projection.session_id,
                                    created_at: chrono::DateTime::parse_from_rfc3339(
                                        &projection.created_at,
                                    )
                                    .map(|dt| dt.timestamp_millis() as u64)
                                    .unwrap_or(0),
                                })
                                .await;
                        }
                        Err(error) => {
                            use sdk::SessionResumeFailureKind;
                            let kind = match error {
                                context::SessionManagementError::NotFound(_) => {
                                    SessionResumeFailureKind::NotFound
                                }
                                context::SessionManagementError::Corrupt(_)
                                | context::SessionManagementError::UnsupportedFutureVersion(_) => {
                                    SessionResumeFailureKind::Corrupt
                                }
                                context::SessionManagementError::Storage(_)
                                | context::SessionManagementError::Resume(_) => {
                                    SessionResumeFailureKind::Io
                                }
                            };
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumeFailed {
                                    kind,
                                    id: id.clone(),
                                    message: error.to_string(),
                                })
                                .await;
                        }
                    }
                    continue;
                }
                PendingCommand::QueryReflectionHistory { limit } => {
                    match list_reflection_history(limit).await {
                        Ok(records) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::ReflectionHistory { records })
                                .await;
                        }
                        Err(e) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("List reflection history failed: {e}"),
                                    is_error: true,
                                })
                                .await;
                        }
                    }
                    continue;
                }
                PendingCommand::ListModels => match list_models().await {
                    Ok(models) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::ModelList { models })
                            .await;
                        continue;
                    }
                    Err(e) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("List models failed: {e}"),
                                is_error: true,
                            })
                            .await;
                        continue;
                    }
                },
                PendingCommand::ListReminders => match list_reminders().await {
                    Ok(reminders) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::ReminderList { reminders })
                            .await;
                        continue;
                    }
                    Err(e) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("List reminders failed: {e}"),
                                is_error: true,
                            })
                            .await;
                        continue;
                    }
                },
            }
        };
    }

            'session: loop {
                // Busy user messages are deliberately adopted one at a time: each starts a distinct Run.
                let idle_result = if !pending_input.is_empty() {
                    // Busy control events are serviced at idle before the next queued user Run. They are
                    // never appended to model context.
                    let next_segment = ChatId::new_v7().to_string();
                    let gate = apply_gate(
                        GateKind::BeforeLlm,
                        &mut pending_input,
                        &sink,
                        task_access.as_ref(),
                        true,
                    )
                    .await;
                    if gate.reset_requested {
                        IdleResult::ResetRequested
                    } else if let Some(command) = gate.pending_command {
                        IdleResult::CommandRequested(command)
                    } else if gate.appended_user_messages > 0 {
                        IdleResult::Resumed(next_segment, gate.adopted_messages)
                    } else {
                        continue;
                    }
                } else if let Some(event) = deferred_user_inputs.pop_front() {
                    pending_input.push(event);
                    let next_segment = ChatId::new_v7().to_string();
                    let gate = apply_gate(
                        GateKind::BeforeLlm,
                        &mut pending_input,
                        &sink,
                        task_access.as_ref(),
                        true,
                    )
                    .await;
                    if gate.reset_requested {
                        IdleResult::ResetRequested
                    } else if let Some(command) = gate.pending_command {
                        IdleResult::CommandRequested(command)
                    } else if gate.appended_user_messages > 0 {
                        IdleResult::Resumed(next_segment, gate.adopted_messages)
                    } else {
                        continue;
                    }
                } else {
                    idle_until_resume_or_shutdown(
                        &input_events,
                        &sink,
                        &mut pending_input,
                        task_access.as_ref(),
                    )
                    .await
                };

                let segment_id = match idle_result {
                    IdleResult::Shutdown => break 'session,
                    IdleResult::ResetRequested => {
                        let bound = match wiring.bind_main_run().await {
                            Ok(bound) => bound,
                            Err(error) => {
                                sink.send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Session reset 失败：{error}"),
                                    is_error: true,
                                }).await;
                                continue;
                            }
                        };
                        let session_id = crate::ports::SessionId::new(bound.session().id.clone());
                        let coordinator = crate::application::context_coordination::ContextCoordinator::new(bound.context());
                        match coordinator.clear_session(&session_id).await {
                            Ok(()) => {
                                messages.clear();
                                sink.send_event(RuntimeStreamEvent::SessionReset).await;
                            }
                            Err(error) => {
                                sink.send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Session reset 失败：{error}"),
                                    is_error: true,
                                }).await;
                            }
                        }
                        continue;
                    }
                    IdleResult::CommandRequested(command) => handle_pending_command!(command),
                    IdleResult::Resumed(next_segment, adopted) => {
                        // 新 Run 只取得本轮 adopted 输入；已提交历史由 Context backing 提供。
                        messages = initial_git_context
                            .take()
                            .into_iter()
                            .chain(adopted.into_iter().map(|(_, message)| message))
                            .collect();
                        next_segment
                    }
                };

                turn_count += 1;
                let turn_id = ChatTurnId::new_v7();
                let turn_context = RuntimeTurnContext::new(chat_id.clone(), turn_id.clone());
                sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
                    .await;
                let started_at = Instant::now();
                cwd = workspace.read().current_workspace_root();

                let text = messages
                    .last()
                    .map(|message| message.text_content())
                    .unwrap_or_default();
                let observation =
                    reasoning.observe(ReasoningSignal::UserMessage { text, turn_count });
                if observation.changed() {
                    sink.send_event(RuntimeStreamEvent::GraphPhaseChanged {
                        node: observation.current,
                        effort: observation.requested,
                        prev: observation.previous,
                    })
                    .await;
                }

                handle_turn_boundary_config(
                    &mut config_snapshot,
                    turn_count,
                    &sink,
                    &mut messages,
                    &language,
                    &segment_id,
                )
                .await;

                let bound_main_run = match wiring.bind_main_run().await {
                    Ok(bound) => bound,
                    Err(error) => {
                        log::error!(target: crate::LOG_TARGET, "main run bind failed: {error}");
                        continue;
                    }
                };
                let bound_session_id = bound_main_run.session().id.clone();
                let context = crate::application::context_coordination::ContextCoordinator::new(
                    bound_main_run.context(),
                );
                if session_id != bound_session_id {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "Runtime Session ID 与 Context 提交状态不一致，采用 Context Session ID"
                    );
                    session_id = bound_session_id;
                }
                let cancel = CancellationToken::new();
                let mut run = Run::new(RunSpec::main(), None);
                let run_memory = bound_main_run.memory_arc();
                let run_memory_config = bound_main_run.config().memory().clone();
                let run_id = run.id().clone();
                active_run.activate(run_id.clone(), cancel.clone());
                let cacheable_system_prompt = system_blocks
                    .iter()
                    .map(|block| block.text())
                    .chain((!user_context.is_empty()).then_some(user_context.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let mut port = MainRunPort {
                    messages: messages.clone(),
                    step_messages: main_run_port::StepMessageOwnership::new(messages.clone()),
                    sink: &sink,
                    queue: &queue,
                    input_events: &input_events,
                    binding: &binding,
                    tool_catalog: &tool_catalog,
                    tool_execution: &tool_execution,
                    tool_context_binding: &tool_context_binding,
                    system_prompt_text: &cacheable_system_prompt,
                    config_snapshot: bound_main_run.config(),
                    context: &context,
                    context_request: None,
                    context_window: None,
                    context_size,
                    workspace: &workspace,
                    session_id: &session_id,
                    read_files: &read_files,
                    session_reminders: &session_reminders,
                    agent_runner: &agent_runner,
                    tool_result_materializer: tool_result_materializer.as_ref(),
                    policy: policy.as_ref(),
                    task_access: &task_access,
                    max_tool_concurrency,
                    agent_semaphore: &agent_semaphore,
                    hook_runner: &hook_runner,
                    memory_config: &run_memory_config,
                    memory: &run_memory,
                    reflection_history: &reflection_history,
                    reflection_tasks: &reflection_tasks,
                    language: &language,
                    reasoning: reasoning.as_ref(),
                    pending_input: &mut pending_input,
                    deferred_user_inputs: &mut deferred_user_inputs,
                    stop_hook_feedback: None,
                    cancel: cancel.clone(),
                    run_id: run_id.clone(),
                    active_run: active_run.as_ref(),
                    turn_count,
                    turn_context,
                    last_total_tokens: &mut last_total_tokens,
                    task_reminder_state: &mut task_reminder_state,
                    tool_identity: &tool_identity,
                    started_at,
                };
                let run_result = logging::within(
                    logging::LogContextPatch {
                        turn: logging::FieldPatch::Set(turn_count),
                        ..logging::LogContextPatch::default()
                    },
                    run_loop(&mut run, &cancel, &mut port),
                )
                .await;
                if let Err(error) = run_result {
                    log::error!(target: crate::LOG_TARGET, "main shared run loop failed: {error}");
                }
                // Runtime 不保留跨 Run 的语义消息；已提交历史只存在于 Context backing。
                messages.clear();
                active_run.clear(&run_id);
            }
            // Session teardown first drains within a bounded grace period. If a
            // Reflection job is still active, shutdown cancels it and waits for
            // its terminal durable record before the Run lease is released.
            let _ = reflection_tasks.shutdown(std::time::Duration::from_secs(5)).await;
        },
    )
    .await
}
