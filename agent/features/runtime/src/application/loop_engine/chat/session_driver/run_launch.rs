use std::sync::Arc;

use sdk::ids::{ChatId, ChatRunId};
use share::message::Message;

use super::super::loop_context::SessionCommandDriverInput;
use super::super::main_run_port;
use super::run_preparation::{create_main_run, prepare_main_run};
use crate::application::loop_engine::chat::idle_lifecycle::{
    execute_set_thinking, idle_until_resume_or_shutdown, IdleResult,
};
use crate::application::loop_engine::chat::input_gate::apply_gate;
use crate::application::loop_engine::chat::loop_phases::handle_turn_boundary_config;
use crate::application::loop_engine::chat::{
    ChatEventSink, GateKind, PendingCommand, PendingInputBuffer, RuntimeRunContext,
    RuntimeStreamEvent,
};
use crate::domain::agent_run::RunSpec;

/// Drives one Main session across idle commands and sequential Runs.
/// The session itself only idles, accepts one real user input, creates one fresh `Run`,
/// drives it to a terminal state through the shared engine, then idles again.
/// `Run` is the only production state machine inside an active run step.
pub async fn run_session_command_driver<S, I>(input: SessionCommandDriverInput<S, I>)
where
    S: ChatEventSink,
    I: crate::application::loop_engine::input_strategy::SessionInputPort,
{
    let session_id_for_scope = input.session.session_snapshot().session_id().to_string();
    let chat_id = ChatId::new_v7();
    logging::within(
        logging::LogContextPatch {
            session_id: logging::FieldPatch::Set(session_id_for_scope),
            chat_id: logging::FieldPatch::Set(chat_id.to_string()),
            ..logging::LogContextPatch::default()
        },
        async move {
            let SessionCommandDriverInput {
                sink,
                input_events,
                session: shell,
                read_files,
                session_queries,
            } = input;

            // #1385 TaskData 12: Construct real ChatEventSinkHandle from session sink.
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
            let agent_runner: Option<Arc<dyn tools::published::agent::AgentRunner>> =
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
            let mut messages = Vec::new();
            let mut initial_git_context = (!initial_git_context.is_empty())
                .then_some(Message::system_generated_user(initial_git_context));
            // Interval and PreCompact share this single session-scoped slot.
            let reflection_tasks =
                crate::application::reflection::ReflectionTaskAdapter::production(
                    std::time::Duration::from_secs(120),
                );
            let mut cwd = workspace.read().current_workspace_root();
            // Per-Session usage tracker shared across all Main Runs.
            let session_usage = crate::application::run::context::RunUsageTracker::new();
            let mut run_count = 0;
            let mut pending_input = PendingInputBuffer::default();
            // idle `/compact` 已受理，等待下一次循环启动手动压缩 Run。
            let mut manual_compaction_requested = false;
            // idle `/reflect-now` 已受理（配置门禁已过），等待下一次循环启动手动反思 Run。
            let mut manual_reflection_requested = false;
                let tool_identity =
                    crate::application::tool::coordination::identity::ToolIdentityRegistry::new();
            let mut config_snapshot =
                crate::application::loop_engine::chat::config_reload::init_snapshot_registry(&cwd);

            // Model switching is assembled from session-owned config and provider factories.
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
                        log::debug!(
                            target: crate::LOG_TARGET,
                            "[compact] idle command accepted; 启动手动压缩 Run"
                        );
                        manual_compaction_requested = true;
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
                    let (text, is_error) = super::super::idle_commands::execute_init(&cwd_str, force);
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
                                super::super::idle_commands::execute_session(
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
                            super::super::idle_commands::execute_memory(&args, memory.as_ref(), &config)
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
                            // 会话身份已切换：emit SessionStart 让外部集成重新捕获
                            // 恢复后的会话 id（生命周期点，失败不阻断恢复流程）。
                            crate::application::hook::session_start::emit_session_start(
                                &shell.runtime_context_factory.services().hooks,
                                &workspace.read().current_workspace_root(),
                                &session_id,
                            )
                            .await;
                            // Run 计数器（Reflection interval 频控）per-session：
                            // resume 切换 session 后从 0 重数，NEVER 延续旧 session 计数。
                            run_count = 0;
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
                                        .map(|step| super::super::RuntimeResumedSessionStep {
                                            run_id: step.run_id,
                                            step_id: step.step_id,
                                            message_segments: step.message_segments,
                                            finalize_cause: step.finalize_cause,
                                            duration_ms: step.duration_ms,
                                        })
                                        .collect(),
                                    display_history: resume_view.display_history,
                                    session_id: resume_view.session_id,
                                    created_at: chrono::DateTime::parse_from_rfc3339(
                                        &resume_view.created_at,
                                    )
                                    .map(|dt| dt.timestamp_millis() as u64)
                                    .unwrap_or(0),
                                    compacted: resume_view.compacted,
                                })
                                .await;
                            let task_state = crate::application::loop_engine::chat::task_snapshot::build_task_state_view(
                                &*task_access,
                                session_id.as_str(),
                            );
                            sink.send_event(RuntimeStreamEvent::TaskStateChanged {
                                state: Box::new(task_state),
                            })
                            .await;                        }
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
                PendingCommand::QueryReflectionHistory { limit } => {                    match session_queries.list_reflection_history(limit).await {
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
                // `/reflect-now` 不再裸 await 反思——idle 受理先做
                // 配置门禁（关闭时直接回「未启用」文案，不创建 Run、不产生 activity）；
                // 开启时只置标志，下一次循环创建 ManualReflection intent 的真实 Run，
                // 消息快照在 run launch 装配前从 committed session 冻结。
                PendingCommand::ReflectNow => {
                    if !crate::application::loop_engine::chat::reflection::reflection_enabled(
                        &memory_config,
                    ) {
                        let (text, is_error) = crate::application::loop_engine::chat::reflection::manual_reflection_outcome_text(
                            &crate::application::reflection::ReflectionRunOutcome::DisabledSkipped,
                        );
                        sink.send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                        continue;
                    }
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[reflect-now] idle command accepted; 启动手动反思 Run"
                    );
                    manual_reflection_requested = true;
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
            }
        };
    }

            'session: loop {
                // Busy user messages are no longer deferred to the session. They
                          // accumulate in the Run-scoped buffer and are consumed within the
                          // same Run (#1272).
                          let idle_result = if manual_compaction_requested {
      IdleResult::ManualCompactionRequested
    } else if manual_reflection_requested {
      IdleResult::ManualReflectionRequested
    } else if !pending_input.is_empty() {
                                  // 裁决 3：busy 期间积压的 `/reflect-now` NEVER 排队执行——
                                  // 在 gate 消费 pending buffer 前统一丢弃并逐条提示（gate
                                  // requeue 残留的拦截点；busy 积压的主拦截点在 Run 收尾的
                                  // drain_remaining_events；idle 受理路径不经此分支）。
                                  crate::application::loop_engine::chat::input_gate::drop_queued_reflect_now(
                                      &pending_input,
                                      &sink,
                                  );
                                  if pending_input.is_empty() {
                                      continue;
                                  }
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
                                      accepted_inputs: gate.accepted_inputs,
                                  }                              } else {
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

                let manual_compaction_run =
                    matches!(idle_result, IdleResult::ManualCompactionRequested);
                let manual_reflection_run =
                    matches!(idle_result, IdleResult::ManualReflectionRequested);
                // 手动反思的材料快照必须在 run launch 装配前取自当前 committed session
                // 的可见结构化消息（对齐原 ReflectNow 分支的取法），不能用 Run buffer 猜历史。
                let mut manual_reflection_messages: Vec<Message> = Vec::new();
                // 反思游标推进基准（#1827）：Manual 快照时 session 历史总长。
                let mut manual_reflection_coverage_end: Option<u64> = None;
                let (segment_id, accepted_inputs) = match idle_result {
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
                                // Run 计数器（Reflection interval 频控）绑定
                                // session epoch：/clear 即新 epoch，从 0 重数。
                                run_count = 0;
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
                    IdleResult::ManualCompactionRequested => {
                        manual_compaction_requested = false;
                        (ChatId::new_v7().to_string(), Vec::new())
                    }
                    IdleResult::ManualReflectionRequested => {
                        manual_reflection_requested = false;
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
                        // 游标增量切片（#1827）：有效游标 → 只带增量；缺失/失效
                        // （首次或 compact 截断）→ 回退全量。增量为空（上次反思后无
                        // 新对话）→ 跳过执行，NEVER 为空跑支付 token。
                        let history_messages = bound.session().structured_messages();
                        let history_len = history_messages.len() as u64;
                        let cursor = crate::application::loop_engine::chat::reflection::latest_coverage_cursor(
                            &shell.runtime_context_factory.services().reflection_history,
                        )
                        .await;
                        match crate::application::loop_engine::chat::reflection::slice_increment_since_cursor(
                            &history_messages,
                            cursor,
                        ) {
                            Some(increment) if increment.is_empty() => {
                                sink.send_event(RuntimeStreamEvent::CommandResultText {
                                    text: "自上次反思以来无新增对话内容，已跳过本次手动反思。"
                                        .to_string(),
                                    is_error: false,
                                })
                                .await;
                                continue;
                            }
                            Some(increment) => {
                                manual_reflection_messages = increment;
                            }
                            None => {
                                manual_reflection_messages = history_messages;
                            }
                        }
                        // 游标推进基准 = 快照时历史总长（增量与回退全量同值）。
                        manual_reflection_coverage_end = Some(history_len);
                        (ChatId::new_v7().to_string(), Vec::new())
                    }
                    IdleResult::Resumed {
                        segment_id: next_segment,
                        accepted_inputs,
                    } => {
                        // 轮次边界重扫 skill 目录：上一轮结束后磁盘上的
                        // skill 变更经 SkillsUpdated 事件刷新 TUI slash 目录。
                        shell.skill_refresh.refresh(&sink).await;
                        // 新 Run 只取得本轮 accepted 输入；已提交历史由 Context backing 提供。
                        messages = initial_git_context
                            .take()
                            .into_iter()
                            .chain(accepted_inputs.iter().map(|input| input.model_message()))
                            .collect();
                        (next_segment, accepted_inputs)
                    }                };

                // 硬约束：手动反思不增加 session `run_count`，也不发 `RunChanged`
                // （它不是用户回合，不消耗 Interval 反思的频控计数）。
                if !manual_reflection_run {
                    run_count += 1;
                    sink.send_event(RuntimeStreamEvent::RunChanged(run_count))
                        .await;
                }
                let run_id = ChatRunId::new_v7();
                let turn_context = RuntimeRunContext::new(chat_id.clone(), run_id.clone());
                cwd = workspace.read().current_workspace_root();
                shell
                    .session_state
                    .write()
                    .unwrap_or_else(|error| error.into_inner())
                    .update_workspace(cwd.clone());

                // returns Ready with user input.

                let config_reader = wiring.config_reader();
                let turn_boundary_config = handle_turn_boundary_config(
                    &mut config_snapshot,
                    config_reader.as_ref(),
                    wiring.as_ref(),
                    run_count,
                    &sink,
                    &language,
                    &segment_id,
                )
                .await;
                let _refresh = &turn_boundary_config.refresh;
                let preparation = match prepare_main_run(
                    &shell,
                    &wiring,
                    &reasoning,
                    &sink_handle,
                    &session_usage,
                    if manual_compaction_run {
                        RunSpec::manual_compaction()
                    } else if manual_reflection_run {
                        RunSpec::manual_reflection()
                    } else {
                        RunSpec::main()
                    },
                ) {
                    Ok(preparation) => preparation,
                    Err(error) => {
                        log::error!(target: crate::LOG_TARGET, "main run preparation failed: {error}");
                        continue;
                    }
                };
                log::debug!(target: crate::LOG_TARGET,                    "[config] starting main run with revision={} allow_all={} session_revision={}",
                    preparation.run_config.revision().get(),
                    preparation.run_config.allow_all(),
                    preparation.request.session().revision(),
                );
                let mut run_instance = match create_main_run(&shell, preparation) {
                    Ok(instance) => instance,
                    Err(error) => {
                        log::error!(target: crate::LOG_TARGET, "main run creation failed: {error}");
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
                run_instance.initialize(messages.clone(), run_count);
                let runtime_context = run_instance.context().clone();
                let run_id = run_instance.run().id().clone();
                let spec = run_instance.run().spec().clone();

                let cancel = runtime_context.cancel().token().clone();
                let heartbeat_cancel = tokio_util::sync::CancellationToken::new();
                let heartbeat_task = {
                    let heartbeat_cancel = heartbeat_cancel.clone();
                    let registry = runtime_context.published_state();
                    let sink = runtime_context.event_sink();
                    let activities = runtime_context.activities().clone();
                    tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_secs(1));
                        interval.tick().await;
                        loop {
                            tokio::select! {
                                _ = heartbeat_cancel.cancelled() => break,
                                _ = interval.tick() => {
                                    if let Some(status) = registry.heartbeat() {
                                        activities.publish_heartbeat();
                                        sink.try_send_event(RuntimeStreamEvent::RuntimeStatusChanged {
                                            status: Box::new(status),
                                        });
                                    } else {
                                        activities.publish_heartbeat();
                                    }
                                }
                            }
                        }
                    })
                };
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
                // The idle gate consumed and typed the initial user input.
                // Seed the Run buffer with that canonical input exactly once;
                // freeze/adoption derive from the same object without rebuilding metadata.
                for input in accepted_inputs {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[loop_debug] idle_initial seeding run_input_buffer id={} source={:?}",
                        input.input_id(),
                        input.model_message().source(),
                    );
                    input_source
                        .run_input_buffer
                        .with_lock(|buffer| buffer.push_accepted(input));
                }                // #1280: Main Run creation, ActiveRun registration, shared
                // run_loop and cleanup are all owned by RunLauncher.
                // await_user_input is handled inside the Main input strategy (async park
                // on input_events channel), so run_loop only returns Terminal.
                let main_active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort> =
                    active_run.clone();

                // #1385 TaskData 7: Install parent frame via RAII guard so sub-agent
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
                // Reminder 统一管线（07-reminder-pipeline.md）：数据获取在
                // Runtime、注入决策在 Context（placement / dedup / 预算 /
                // compact 处置）。sources 按启动期事实条件注册。
                let mut reminder_sources: Vec<std::sync::Arc<dyn context::ReminderSource>> = vec![
                    std::sync::Arc::new(
                        crate::application::loop_engine::chat::reminder_sources::TaskProgressReminderSource::new(
                            runtime_context.task(),
                            share::config::TaskListConfig::default().max_lines,
                        ),
                    ),
                ];
                if !turn_boundary_config.guidance_changed_paths.is_empty() {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "reminder_source_registered kind=guidance_sources_changed trigger=turn_boundary_config paths={:?}",
                        turn_boundary_config.guidance_changed_paths,
                    );
                    reminder_sources.push(std::sync::Arc::new(
                        crate::application::loop_engine::chat::reminder_sources::RunStartFactReminderSource::guidance_sources_changed(
                            turn_boundary_config.guidance_changed_paths.clone(),
                            runtime_context
                                .config_ref()
                                .config()
                                .guidance_reload_policy(),
                        ),
                    ));
                }
                if runtime_context.provider_ref().model.model != shell.prompt_model_id {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "reminder_source_registered kind=model_guidance_mismatch session_model={} run_model={}",
                        shell.prompt_model_id,
                        runtime_context.provider_ref().model.model,
                    );
                    reminder_sources.push(std::sync::Arc::new(
                        crate::application::loop_engine::chat::reminder_sources::RunStartFactReminderSource::model_guidance_mismatch(
                            shell.prompt_model_id.clone(),
                            runtime_context.provider_ref().model.model.clone(),
                        ),
                    ));
                }
                if let Some(notice) = reflection_tasks.take_memory_update_notice() {
                    // The TUI already showed the notice when reflection finished;
                    // the model needs the same facts in the turn that follows it.
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "reminder_source_registered kind=memory_updated changed={}",
                        notice.changed,
                    );
                    reminder_sources.push(std::sync::Arc::new(
                        crate::application::loop_engine::chat::reminder_sources::RunStartFactReminderSource::memory_updated(notice.changed),
                    ));
                }
                // per-message 记忆召回（#1834）：开关开且评分端口已装配时注册 source
                // 并写入 slot（accept_step_input 在 turn 边界 refresh 预物化）。
                if let Some(scorer) = runtime_context.scoring() {
                    if runtime_context.config_ref().config().scoring().memory_recall {
                        let recall_source = std::sync::Arc::new(
                            crate::application::loop_engine::chat::reminder_sources::MemoryRecallReminderSource::new(
                                runtime_context.memory(),
                                scorer,
                            ),
                        );
                        log::debug!(
                            target: crate::LOG_TARGET,
                            "reminder_source_registered kind=memory_recall trigger=on_user_message",
                        );
                        reminder_sources.push(recall_source.clone());
                        if runtime_context.memory_recall_slot().set(recall_source).is_err() {
                            log::warn!(
                                target: crate::LOG_TARGET,
                                "memory_recall slot 已占用（重复装配），本次 source 丢弃"
                            );
                        }
                    }
                }
                let reminder_context_port = runtime_context.context();
                reminder_context_port.create_reminder_pipeline(
                    run_id.clone(),
                    std::mem::take(&mut reminder_sources),
                );
                reminder_context_port.reminder_run_started(&run_id);
                let context_request =
                    crate::application::loop_engine::run_services::ContextRequest {
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
                let tool_agent = main_run_port::make_agent(
                    &runtime_context,
                    agent_runner.clone(),
                    &language,
                    &workspace,
                    &cancel,
                    read_files.clone(),
                    max_tool_concurrency,
                    agent_semaphore.clone(),
                    &session_id,
                    &run_id,
                    tool_result_materializer.clone(),
                );

                // #1494：边流边执行句柄——流中 ToolCallCompleted 即旁路执行工具。
                // 与工具轮次共享 policy/hook/并发编排；结果缓冲由 engine Tools 阶段统一汇总。
                let streaming_tool =
                    Arc::new(crate::application::loop_engine::chat::streaming_tool::StreamingToolExecutor::new(
                        Arc::new(runtime_context.clone()),
                        tool_agent.clone(),
                        turn_context.clone(),
                        run_id.clone(),
                        language.clone(),
                        workspace.read(),
                        max_tool_concurrency,
                    ));
                let model_observer = main_run_port::ChatModelObserver {
                    runtime_context: runtime_context.clone(),
                    input: input_source.clone(),
                    context_size,
                    turn_context: turn_context.clone(),
                    tool_identity: tool_identity.clone(),
                    streaming_tool: Some(streaming_tool),
                };
                let mut model =
                    crate::application::loop_engine::run_services::RuntimeModelInvocation::new(
                        model_observer,
                        false,
                    );
                // PreCompact 材料共享槽：压缩观察者在 Committed 时暂存将被丢弃的
                // 消息，反思端口在 Compacting 内取出执行（材料收集与状态机分离）。
                let pre_compact_material =
                    crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot::default();
                // Interval 反思材料槽（#1827 游标增量）：读游标切片 session 历史增量
                // 装槽；游标缺失（首次）或失效（compact 截断）时装空槽，Interval 回退为
                // 仅当前 Run 消息（不推进游标）。单用途 Run（手动压缩/反思）不装槽。
                let interval_material =
                    crate::application::loop_engine::chat::reflection::IntervalReflectionMaterialSlot::default();
                if !manual_reflection_run && !manual_compaction_run {
                    if let Ok(bound) = wiring.bind_main_run().await {
                        let history_messages = bound.session().structured_messages();
                        let cursor = crate::application::loop_engine::chat::reflection::latest_coverage_cursor(
                            &shell.runtime_context_factory.services().reflection_history,
                        )
                        .await;
                        if let Some(increment) = crate::application::loop_engine::chat::reflection::slice_increment_since_cursor(
                            &history_messages,
                            cursor,
                        ) {
                            interval_material.stage(increment, history_messages.len() as u64);
                        }
                    }
                }
                let mut compaction =
                    crate::application::loop_engine::run_services::RuntimeCompaction::new(
                        &runtime_context,
                        main_run_port::ChatCompactionObserver {
                            pre_compact_material: pre_compact_material.clone(),
                        },
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
                            workspace.read(),
                            session_id.clone(),
                            language.clone(),
                        ),
                        main_run_port::ChatStopHookObserver {
                            sink: runtime_context.event_sink(),
                            continuation: input_continuation.clone(),
                        },
                    );
                let tool_context = crate::application::tool::coordination::ToolRoundContext {
                    runtime_context: &runtime_context,
                    agent: tool_agent,
                    turn_context: turn_context.clone(),
                    language: &language,
                    workspace_read: workspace.read(),
                    session_id: &session_id,
                    materializer: tool_result_materializer.as_ref(),
                    log_patch: logging::LogContextPatch::default(),
                };
                let mut tools =
                    crate::application::loop_engine::run_services::RuntimeToolOrchestration::new(
                        tool_context,
                        main_run_port::ChatToolRoundObserver {
                            runtime_context: runtime_context.clone(),
                            workspace_read: workspace.read(),
                            turn_context: turn_context.clone(),
                            session_id: session_id.clone(),
                            materializer: tool_result_materializer.clone(),
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
                let mut manual_compaction = main_run_port::ChatManualCompaction {
                    runtime_context: runtime_context.clone(),
                    session_id: session_id.clone(),
                    system_prompt: cacheable_system_prompt.clone(),
                    context_size,
                };
                // 手动反思端口：messages 是 idle 受理时冻结的 committed 快照；
                // accepted_inputs 为空也必须装配（端口的空输入仍被反思执行使用）。
                let mut manual_reflection = main_run_port::ChatManualReflection {
                    runtime_context: runtime_context.clone(),
                    reflection_tasks: reflection_tasks.clone(),
                    system_prompt: system_prompt_text.clone(),
                    language: language.clone(),
                    messages: manual_reflection_messages,
                    coverage_end: manual_reflection_coverage_end,
                };
                // Main Run 都绑反思端口——Interval/PreCompact 反思的判定与执行
                // 由 engine reflection phase 驱动。
                let mut reflection =
                    crate::application::loop_engine::run_services::RuntimeReflection::new(
                        &runtime_context,
                        reflection_tasks.clone(),
                        cacheable_system_prompt.clone(),
                        language.clone(),
                        pre_compact_material,
                        interval_material,
                    );
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
                );
                loop_context.bind_reflection(&mut reflection);
                if manual_compaction_run {
                    loop_context.bind_manual_compaction(&mut manual_compaction);
                }
                if manual_reflection_run {
                    loop_context.bind_manual_reflection(&mut manual_reflection);
                }
                // `run_step` stays unset at Run level: it is the schema's LLM
                // step counter, and a Run has none. `run_services` sets it per
                // LLM call; binding the Run ordinal here made one field mean
                // two different things.
                let launch_result = crate::application::run::launcher::launch(
                    &mut run_instance,
                    cancel.clone(),
                    main_active_run.clone(),
                    &mut loop_context,
                )
                .await;
                heartbeat_cancel.cancel();
                let _ = heartbeat_task.await;

                  // #1385 TaskData 7: Guard is dropped when the block ends,
                  // clearing only the generation we installed.

                match launch_result {
                    crate::application::run::launcher::RunLaunchResult::Terminal => {}
                    crate::application::run::launcher::RunLaunchResult::Failed(error) => {
                        log::error!(target: crate::LOG_TARGET, "main shared run loop failed: {error}");
                    }
                }
                // Reminder 管线句柄随 Run 销毁：Run-scoped 状态不跨 Run 泄漏
                //（07-reminder-pipeline.md）。
                runtime_context
                    .context()
                    .drop_reminder_pipeline(&run_id);
                // Return any remaining Run-scoped events (control commands
                // buffered during await_user_input) to the session idle gate.
                input_source.drain_remaining_events();
                // Runtime 不保留跨 Run 的语义消息；已提交历史只存在于 Context backing。
                messages.clear();
            }
            // Reflection is a synchronous stage inside the Run that owns it, so
            // teardown has no background job to drain: every run has already
            // reached a terminal durable record before this point.
        },
    )
    .await
}
