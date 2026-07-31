use std::sync::Arc;

use sdk::ids::{ChatId, ChatTurnId};
use share::message::Message;

use crate::application::loop_engine::chat::idle_lifecycle::{
    execute_set_thinking, idle_until_resume_or_shutdown, IdleResult,
};
use crate::application::loop_engine::chat::input_gate::apply_gate;
use crate::application::loop_engine::chat::loop_phases::handle_turn_boundary_config;
use crate::application::loop_engine::chat::{
    ChatEventSink, GateKind, PendingCommand, PendingInputBuffer, RuntimeStreamEvent,
    RuntimeTurnContext,
};
use crate::domain::agent_run::RunSpec;

use super::loop_context::ChatLoopContext;

#[path = "main_run_port.rs"]
pub(crate) mod main_run_port;

/// Session actor for Main chat. The session itself only idles, accepts one real user input,
/// creates one fresh `Run`, drives it to a terminal state through the shared engine, then idles
/// again. `Run` is the only production state machine inside an active turn.
pub async fn process_chat_loop<S, I>(ctx: ChatLoopContext<S, I>)
where
    S: ChatEventSink,
    I: crate::application::loop_engine::input_strategy::SessionInputPort,
{
    let session_id_for_scope = ctx.shell.session_snapshot().session_id().to_string();
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
                input_events,
                shell,
                read_files,                session_reminders,
                session_queries,
            } = ctx;

            // #1385 Task 12: Construct real ChatEventSinkHandle from session sink.
            // The handle is Clone and can be used in place of S everywhere.
            let sink_handle =
                crate::application::loop_engine::chat::ChatEventSinkHandle::new(sink.clone());

            // ── #1385: Compute session-level locals from shell (single source of truth) ──
            let initial_git_context = shell.initial_git_context.clone();
            let user_context = shell.user_context.clone();
            let system_blocks = shell.system_blocks.clone();
            let system_prompt_text = shell.system_prompt_text.clone();
            let workspace = shell.workspace.clone();
            let wiring = shell.wiring.clone();
            let tool_result_materializer = shell.tool_result_materializer.clone();
            let agent_runner: Option<Arc<dyn tools::AgentRunner>> =
                Some(shell.agent_runner.clone());
            let max_tool_concurrency = shell.max_tool_concurrency;
            let agent_semaphore = shell.agent_semaphore.clone();
            let language = shell.language.clone();
            let memory_config = shell.memory_config.clone();
            let active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort> =
                shell.active_run.clone();
            let provider_factory = shell.provider_factory.clone();
            let config_query_for_switch = shell.config_query.clone();
            let task_access = shell.runtime_context_factory.services().task.clone();

            let binding = shell.model_state.binding();
            let reasoning = Arc::new(std::sync::Mutex::new(binding.requested_reasoning));
            let session_snapshot = shell.session_snapshot();
            let mut context_size = shell.context_size;
            let mut session_id = session_snapshot.session_id().to_string();
            let mut messages = Vec::new();            let mut initial_git_context = (!initial_git_context.is_empty())
                .then_some(Message::system_generated_user(initial_git_context));
            // Interval and PreCompact share this single session-scoped slot.
            let reflection_tasks =
                crate::application::reflection::ReflectionTaskAdapter::production(
                    std::time::Duration::from_secs(120),
                );
            let mut cwd = workspace.read().current_workspace_root();
            // #1385 Task 12: last_total_tokens eliminated — usage tracker is per-Run via RuntimeContext.
            let mut turn_count = 0;
            let mut pending_input = PendingInputBuffer::default();
                let tool_identity =
                    crate::application::tool::coordination::identity::ToolIdentityRegistry::new();
            let mut config_snapshot =
                crate::application::loop_engine::chat::config_reload::init_snapshot_registry(&cwd);

            // #1385: build_switched_client is now computed from shell fields in loop_runner
            // (was previously in trait_chat.rs ChatLoopContext construction)
            let build_switched_client: crate::application::loop_engine::chat::loop_context::SwitchClientFn = {
                let config_query = config_query_for_switch.clone();
                std::sync::Arc::new(move |selection: &str| {
                    let selection = selection.to_string();
                    let config_query = config_query.clone();
                    let provider_factory = provider_factory.clone();
                    Box::pin(async move {
                        crate::application::client::trait_model::build_provider_binding_for_switch(
                            &selection,
                            config_query.as_ref(),
                            provider_factory.as_ref(),
                        )
                        .await
                    })
                })
            };
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
                    let coordinator = crate::application::context::coordination::ContextCoordinator::new(bound.context());
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
                            *reasoning.lock().unwrap_or_else(|error| error.into_inner()) =
                                new_binding.requested_reasoning;
                            let committed_config = shell.wiring.committed_config();
                            shell
                                .session_state
                                .write()
                                .unwrap_or_else(|error| error.into_inner())
                                .update_provider_binding(&new_binding, committed_config);
                            shell.model_state.update_binding(Arc::new(new_binding));
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
                        match session_queries.list_sessions().await {
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
                        let project = wiring.project_identity();
                        let args = args.clone();
                        let deleted_session = args.trim_start().starts_with("delete ");
                        let active_session_id = session_id.clone();
                        let result = wiring
                            .with_shared(async move {
                                super::idle_commands::execute_session(
                                    &args,
                                    &active_session_id,
                                    &project,
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
                        if !is_error && deleted_session {
                            match session_queries.list_sessions().await {
                                Ok(sessions) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::SessionList { sessions })
                                        .await;
                                }
                                Err(error) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text: format!("List sessions failed: {error}"),
                                            is_error: true,
                                        })
                                        .await;
                                }
                            }
                        }
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
                        Ok(resume_view) => {
                            session_id = resume_view.session_id.clone();
                            shell
                                .session_state
                                .write()
                                .unwrap()
                                .update_session(
                                    session_id.clone(),
                                    wiring.committed_config(),
                                );
                            messages = resume_view.active_messages.clone();
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumed {
                                    steps: resume_view
                                        .display_steps
                                        .into_iter()
                                        .map(|step| super::RuntimeResumedSessionStep {
                                            run_id: step.run_id,
                                            step_id: step.step_id,
                                            messages: step.messages,
                                            finalize_cause: step.finalize_cause,
                                            duration_ms: step.duration_ms,
                                        })
                                        .collect(),
                                    session_id: resume_view.session_id,
                                    created_at: chrono::DateTime::parse_from_rfc3339(
                                        &resume_view.created_at,
                                    )
                                    .map(|dt| dt.timestamp_millis() as u64)
                                    .unwrap_or(0),
                                })
                                .await;
                        }
                        Err(error) => {
                            use sdk::SessionResumeFailureKind;
                            let kind = match error {
                                context::SessionManagementError::NotFound(_)
                                | context::SessionManagementError::ProjectMismatch(_) => {
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
                    match session_queries.list_reflection_history(limit).await {
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
                PendingCommand::ListModels => match session_queries.list_models().await {
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
                PendingCommand::ListReminders => match session_queries.list_reminders().await {
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
                // Busy user messages are no longer deferred to the session. They
                          // accumulate in the Run-scoped buffer and are consumed within the
                          // same Run (#1272).
                          let idle_result = if !pending_input.is_empty() {
                              // Busy control events are serviced at idle before the next queued user Run. They are
                              // never appended to model context.
                              let next_segment = ChatId::new_v7().to_string();
                              let gate = apply_gate(
                                  GateKind::BeforeLlm,
                                  &pending_input,
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
                                  IdleResult::Resumed {
                                      segment_id: next_segment,
                                      adopted_messages: gate.adopted_messages,
                                      adopted_events: gate.adopted_events,
                                  }
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

                let (segment_id, adopted_events) = match idle_result {
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
                        let coordinator = crate::application::context::coordination::ContextCoordinator::new(bound.context());
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
                    IdleResult::Resumed {
                        segment_id: next_segment,
                        adopted_messages: adopted,
                        adopted_events,
                    } => {
                        // 新 Run 只取得本轮 adopted 输入；已提交历史由 Context backing 提供。
                        messages = initial_git_context
                            .take()
                            .into_iter()
                            .chain(adopted.into_iter().map(|(_, message)| message))
                            .collect();
                        // #1272: seed run_input_buffer with real events (not synthetic)
                        // to preserve InputId and images through the drain→freeze→adopt pipeline.
                        (next_segment, adopted_events)
                    }
                };

                turn_count += 1;
                let turn_id = ChatTurnId::new_v7();
                let turn_context = RuntimeTurnContext::new(chat_id.clone(), turn_id.clone());
                sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
                    .await;
                cwd = workspace.read().current_workspace_root();
                shell
                    .session_state
                    .write()
                    .unwrap_or_else(|error| error.into_inner())
                    .update_workspace(cwd.clone());

                // returns Ready with user input.

                let config_reader = wiring.config_reader();
                let _refresh = handle_turn_boundary_config(
                    &mut config_snapshot,
                    config_reader.as_ref(),
                    wiring.as_ref(),
                    turn_count,
                    &sink,
                    &mut messages,
                    &language,
                    &segment_id,
                )
                .await;
                let run_config = crate::application::run::config::RunConfigSnapshot::capture(
                    wiring.committed_config(),
                );
                let session_snapshot = {
                    let binding = shell.model_state.binding();
                    let mut session_state = shell
                        .session_state
                        .write()
                        .unwrap_or_else(|error| error.into_inner());
                    session_state.update_provider_binding(
                        binding.as_ref(),
                        wiring.committed_config(),
                    );
                    session_state.snapshot_for_run()
                };
                let session_bindings =
                    crate::application::run::creation::SessionRunBindings::new(
                        wiring.clone(),
                        shell.model_state.binding(),
                        shell.interaction_bridge.clone(),
                        reasoning.clone(),
                        sink_handle.clone(),
                    );
                log::debug!(target: crate::LOG_TARGET,                    "[config] starting main run with revision={} allow_all={} session_revision={}",
                    run_config.revision().get(),
                    run_config.allow_all(),
                    session_snapshot.revision(),
                );
                let spec = RunSpec::main();
                let request = match crate::application::run::creation::RunCreationRequest::new(
                    spec.clone(),
                    session_snapshot,
                    None,
                ) {
                    Ok(request) => request,
                    Err(error) => {
                        log::error!(target: crate::LOG_TARGET, "main run preparation failed: {error}");
                        continue;
                    }
                };
                let run_factory = crate::application::run::factory::RunFactory::for_session(
                    shell.runtime_context_factory.clone(),
                    session_bindings,
                );
                let mut run_instance = match run_factory.create(request) {
                    Ok(instance) => instance,                    Err(error) => {
                        log::error!(target: crate::LOG_TARGET, "main run preparation failed: {error}");
                        sink.send_event(RuntimeStreamEvent::CommandResultText {
                            text: format!("无法启动 Run：{error}"),
                            is_error: true,
                        })
                        .await;
                        continue;
                    }
                };
                let prepared_session = run_instance.session().clone();
                if session_id != prepared_session.session_id() {
                    session_id = prepared_session.session_id().to_string();
                    shell
                        .session_state
                        .write()
                        .unwrap_or_else(|error| error.into_inner())
                        .update_session(session_id.clone(), prepared_session.config().clone());
                }
                run_instance.initialize(messages.clone(), turn_count);
                let runtime_context = run_instance.context().clone();
                let run_id = run_instance.run().id().clone();
                let spec = run_instance.run().spec().clone();

                let cancel = runtime_context.cancel().token().clone();
                let cacheable_system_prompt = system_blocks
                    .iter()
                    .map(|block| block.text())
                    .chain((!user_context.is_empty()).then_some(user_context.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n\n");
              let input_continuation =
                    crate::application::loop_engine::input_strategy::InputContinuationState::default();
                let mut input_source =
                    crate::application::loop_engine::input_strategy::BufferedInputAdapter {
                        input_events: input_events.clone(),
                        sink: runtime_context.event_sink(),
                        pending_input: pending_input.clone(),                        run_input_buffer: runtime_context.input(),
                        continuation: input_continuation.clone(),
                        run_id: run_id.clone(),
                    };
                let mut launch_input = input_source.clone();
                // #1272: The idle gate consumed the user input from the channel
                // and placed it in `messages`. Seed the run_input_buffer with                // InputId and images are preserved through drain→freeze→adopt.
                // This ensures drain_input returns Ready (not EmptyAndSealed)
                // on the first drain call, and freeze_step captures the correct
                // (InputId, Message) pairs for accept_step_input's Adopted emission.
                for event in adopted_events {
                      if let sdk::ChatInputEvent::UserMessage { id, text, images } = &event {
                          log::debug!(
                              target: crate::LOG_TARGET,
                              "[loop_debug] idle_initial seeding run_input_buffer id={} text_len={} image_count={}",
                              id, text.len(), images.len()
                          );
                      }
                      input_source.run_input_buffer.with_lock(|buffer| buffer.push(event));
                  }
                // #1280: Main Run creation, ActiveRun registration, shared
                // run_loop and cleanup are all owned by RunLauncher.
                // await_user_input is handled inside the Main input strategy (async park
                // on input_events channel), so run_loop only returns Terminal.
                let main_active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort> =
                    active_run.clone();

                // #1385 Task 7: Install parent frame via RAII guard so sub-agent
                // derivation can read the true parent spec + context.
                // The guard clears its own generation on drop — no manual clear.
                let _parent_frame_guard = shell.parent_context_source.install(Arc::new(
                    crate::application::run::context::ParentRunFrame {
                        run_id: run_id.clone(),
                        spec: spec.clone(),
                        context: Arc::new(runtime_context.clone()),
                    },
                ));

                let accepted_input = main_run_port::ChatAcceptedInputObserver {
                    sink: runtime_context.event_sink(),
                    input: runtime_context.input(),
                };
                let context_request =
                    crate::application::loop_engine::run_services::ContextRequestData {
                        runtime_context: &runtime_context,
                        session_id: &session_id,
                        system_prompt: &cacheable_system_prompt,
                        model_id: &runtime_context.provider_ref().model.model,
                        language: &language,
                        agent_roles: std::collections::HashMap::new(),
                        config: runtime_context.config_ref(),
                        context_size,
                        max_output_tokens: runtime_context.provider_ref().max_tokens as usize,
                        raw_tool_schemas: runtime_context
                            .tool_catalog_ref()
                            .snapshot(
                                &tools::RegistryScopeName::new("main"),
                                &tools::ToolProfileName::new("main-full"),
                            )
                            .map(|snapshot| snapshot.model_schemas())
                            .unwrap_or_default(),
                    };
                let mut persistence =
                    crate::application::loop_engine::run_services::RuntimeStepPersistence::new(
                        run_id.clone(),
                        context_request,
                        input_continuation.take_step_prefix(),
                        accepted_input,
                    );
                let mut events = main_run_port::ChatEventPort {
                    sink: runtime_context.event_sink(),
                    session_id: session_id.clone(),
                    turn_context: turn_context.clone(),
                    task_access: runtime_context.task(),
                    model: runtime_context.provider_ref().model.model.clone(),
                };
                let model_observer = main_run_port::ChatModelObserver {
                    runtime_context: runtime_context.clone(),
                    input: input_source.clone(),
                    system_prompt: cacheable_system_prompt.clone(),
                    context_size,
                    reflection_tasks: reflection_tasks.clone(),
                    language: language.clone(),
                    turn_context: turn_context.clone(),
                    tool_identity: tool_identity.clone(),
                };
                let mut model =
                    crate::application::loop_engine::run_services::RuntimeModelInvocation::new(
                        model_observer,
                        false,
                    );
                let mut compaction =
                    crate::application::loop_engine::run_services::RuntimeCompaction::new(
                        &runtime_context,
                        main_run_port::ChatCompactionObserver {
                            runtime_context: runtime_context.clone(),
                            reflection_tasks: reflection_tasks.clone(),
                            system_prompt: cacheable_system_prompt.clone(),
                            language: language.clone(),
                        },
                    );
                let tool_agent = main_run_port::make_agent(
                    &runtime_context,
                    agent_runner.clone(),
                    &language,
                    &workspace,
                    &cancel,
                    read_files.clone(),
                    session_reminders.clone(),
                    max_tool_concurrency,
                    agent_semaphore.clone(),
                    &session_id,
                    &run_id,
                );
                let mut interaction =
                    crate::application::loop_engine::run_services::RuntimeInteraction::new(
                        crate::application::loop_engine::run_services::ChatInteractionPublisher {
                            runtime_context: &runtime_context,
                            tool_context: tool_agent.ctx.clone(),
                            session_id: &session_id,
                            materializer: tool_result_materializer.as_ref(),
                        },
                    );
                let mut stop_hook =
                    crate::application::loop_engine::run_services::RuntimeStopHook::new(
                        crate::application::hook::stop_coordination::StopHookExecutionContext::new(
                            runtime_context.hooks(),
                            workspace.read().current_workspace_root(),
                            session_id.clone(),
                            language.clone(),
                        ),
                        main_run_port::ChatStopHookObserver {
                            sink: runtime_context.event_sink(),
                            continuation: input_continuation.clone(),
                        },
                    );
                let tool_workspace_root = workspace.read().current_workspace_root();
                let tool_context = crate::application::tool::coordination::ToolRoundContext {
                    runtime_context: &runtime_context,
                    agent: tool_agent,
                    turn_context: turn_context.clone(),
                    language: &language,
                    workspace_root: tool_workspace_root.clone(),
                    session_id: &session_id,
                    materializer: tool_result_materializer.as_ref(),
                    log_patch: logging::LogContextPatch::default(),
                };
                let mut tools =
                    crate::application::loop_engine::run_services::RuntimeToolOrchestration::new(
                        tool_context,
                        main_run_port::ChatToolRoundObserver {
                            runtime_context: runtime_context.clone(),
                            workspace_root: tool_workspace_root,
                        },
                    );
                let control = crate::application::loop_engine::run_ports::ActiveRunControl::new(
                    active_run.as_ref(),
                    &run_id,
                );
                let lifecycle =
                    crate::application::loop_engine::run_ports::ActiveRunLifecycle::new(
                        active_run.as_ref(),
                        crate::application::loop_engine::run_ports::StepScopeRegistration::Active(
                            active_run.as_ref(),
                        ),
                    );
                let mut stuck = crate::application::loop_engine::run_ports::NoopStuckObserver;
                let plan_approval =
                    crate::application::loop_engine::run_ports::FixedPlanApproval::new(false);
                let mut loop_context = crate::application::loop_engine::RunLoop::new(
                    &mut launch_input,
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
                let launch_result = logging::within(
                    logging::LogContextPatch {
                        turn: logging::FieldPatch::Set(turn_count),
                        ..logging::LogContextPatch::default()
                    },
                    crate::application::run::launcher::launch(
                        &mut run_instance,
                        cancel.clone(),
                        main_active_run.clone(),
                        &mut loop_context,
                    ),
                )
                .await;

                  // #1385 Task 7: Guard is dropped when the block ends,
                  // clearing only the generation we installed.

                match launch_result {
                    crate::application::run::launcher::RunLaunchResult::Terminal => {}
                    crate::application::run::launcher::RunLaunchResult::Failed(error) => {
                        log::error!(target: crate::LOG_TARGET, "main shared run loop failed: {error}");
                    }
                }
                // Return any remaining Run-scoped events (control commands
                // buffered during await_user_input) to the session idle gate.
                input_source.drain_remaining_events();
                // Runtime 不保留跨 Run 的语义消息；已提交历史只存在于 Context backing。
                messages.clear();
            }
            // Session teardown first drains within a bounded grace period. If a
            // Reflection job is still active, shutdown cancels it and waits for
            // its terminal durable record before the Run lease is released.
            let _ = reflection_tasks.shutdown(std::time::Duration::from_secs(5)).await;
        },
    )
    .await
}
