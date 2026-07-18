use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use sdk::ids::{ChatId, ChatTurnId};
use tokio_util::sync::CancellationToken;

use crate::application::chat::looping::compact::manual_compact;
use crate::application::chat::looping::compact_outcome::apply_compact_outcome;
use crate::application::chat::looping::hook_ui::HookUi;
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
use crate::LOG_TARGET;
use workflow::api::ReasoningSignal;

use super::loop_context::ChatLoopContext;

#[path = "main_run_port.rs"]
mod main_run_port;
use main_run_port::MainRunPort;

/// Session actor for Main chat. The session itself only idles, accepts one real user input,
/// creates one fresh `Run`, drives it to a terminal state through the shared engine, then idles
/// again. `Run` is the only production state machine inside an active turn.
pub async fn process_chat_loop<S, Q, I>(
    ctx: ChatLoopContext<S, Q, I>,
) -> context::session::ChatChain
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    let ChatLoopContext {
        sink,
        queue,
        input_events,
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        mut chain,
        mut context_size,
        workspace,
        wiring,
        session_id,
        read_files,
        session_reminders,
        agent_runner,
        tool_result_materializer,
        allow_all,
        active_run,
        task_store,
        task_access,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        hook_runner,
        memory_config: _,
        memory_source,
        language,
        frozen_chats,
        active_summary: active_summary_arc,
        reasoning,
        build_switched_client,
        save_chain,
        run_reflection_on_demand,
        apply_reflection_on_demand,
        list_models,
        list_reminders,
        list_sessions,
    } = ctx;
    let mut client = client;
    let hook_ui = HookUi::new(sink.clone());
    let mut cwd = workspace.read().current_workspace_root();
    let memory_cwd = workspace.read().initial_cwd();
    let mut active_summary = active_summary_arc
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    let mut last_total_tokens = None;
    let mut turn_count = 0;
    let mut pending_input = PendingInputBuffer::default();
    let mut deferred_user_inputs = VecDeque::new();
    let mut task_reminder_state = TaskReminderState::new();
    let tool_identity =
        crate::application::chat::looping::tool_identity::ToolIdentityRegistry::new();
    let chat_id = ChatId::new_v7();
    logging::set_current_chat_id(chat_id.to_string());
    let mut config_snapshot =
        crate::application::chat::looping::config_reload::init_snapshot_registry(&cwd);
    macro_rules! handle_pending_command {
        ($cmd:expr) => {
            match $cmd {
                PendingCommand::Compact => {
                    // #871: the *entire* manual compact — memory capture +
                    // precompact reflection + LLM summary + apply_compact_outcome
                    // — must run under the session-switch **shared** permit.
                    //
                    // Previously `with_shared` only captured the `Arc<dyn
                    // MemoryPort>` and then released, so the precompact
                    // reflection / LLM `manual_compact` body ran *outside* the
                    // permit. A concurrent resume (exclusive permit) could swap
                    // memory mid-compact, racing the LLM summary against the
                    // memory snapshot. Now the whole `manual_compact` future is
                    // driven inside `with_shared`, so the shared permit spans
                    // the full body — mirroring the forced-reflection and
                    // ManageMemory paths.
                    //
                    // `async { ... }` (not `async move`) so the future borrows
                    // `sink`, `hook_ui`, `hook_runner`, `chain`,
                    // `active_summary`, etc. by reference for the duration of
                    // the `with_shared(...).await`. The shared permit is held
                    // until the entire body — including the LLM
                    // `manual_compact` call — has completed.
                    let wiring_for_compact = wiring.clone();
                    match wiring
                        .with_shared(async {
                            let memory = wiring_for_compact.committed_memory();
                            // H3: read MemoryConfig from the committed config at
                            // compact time so it reflects any runtime resume.
                            let memory_config_for_compact =
                                wiring_for_compact.committed_config().memory().clone();
                            let messages = chain.messages_flat();
                            if let Some(outcome) = manual_compact(
                                &sink,
                                &hook_ui,
                                &hook_runner,
                                turn_count,
                                &messages,
                                active_summary.as_deref(),
                                &system_prompt_text,
                                context_size,
                                &memory_config_for_compact,
                                memory.as_ref(),
                                &memory_cwd,
                                &client,
                                &language,
                                &cwd,
                            )
                            .await
                            {
                                apply_compact_outcome(
                                    &sink,
                                    outcome,
                                    &mut chain,
                                    &frozen_chats,
                                    &mut active_summary,
                                    &active_summary_arc,
                                )
                                .await;
                            }
                        })
                        .await
                    {
                        Ok(()) => {}
                        Err(error) => {
                            log::warn!(
                                target: LOG_TARGET,
                                "manual compact skipped: session switch gate closed: {error:?}"
                            );
                        }
                    }
                    continue;
                }
                PendingCommand::SwitchModel { selection } => {
                    match (build_switched_client)(&selection).await {
                        Ok((new_client, result)) => {
                            reasoning.reset_default_level(
                                new_client.default_scope().requested_reasoning(),
                            );
                            client = Arc::new(new_client);
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
                        // Gate-aware session read: the export branch calls
                        // `load_session` from disk, which must not race with a
                        // session resume (exclusive permit). `with_shared`
                        // acquires a shared permit for the duration of the call.
                        let (text, is_error) = match wiring
                            .with_shared(super::idle_commands::execute_session(
                                &args,
                                &session_id,
                            ))
                            .await
                        {
                            Ok(result) => result,
                            Err(_) => (
                                "Session is being switched, please retry.".to_string(),
                                true,
                            ),
                        };
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
                    // #871: the *entire* memory query/mutation must run under
                    // the session-switch gate. Capturing the Arc<dyn MemoryPort>
                    // and then releasing the permit before the mutation completes
                    // would let a concurrent resume (exclusive permit) swap
                    // memory mid-operation. The whole execute_memory future is
                    // driven inside with_shared so the shared permit spans the
                    // full mutation (write/delete/list), not just the Arc clone.
                    let wiring_for_memory = wiring.clone();
                    match wiring
                        .with_shared(async move {
                            let memory = wiring_for_memory.committed_memory();
                            // H3: read MemoryConfig from committed config so
                            // memory commands reflect any runtime resume.
                            let memory_config_for_memory =
                                wiring_for_memory.committed_config().memory().clone();
                            super::idle_commands::execute_memory(
                                &args,
                                memory.as_ref(),
                                &memory_config_for_memory,
                            )
                            .await
                        })
                        .await
                    {
                        Ok((text, is_error)) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        Err(error) => {
                            log::warn!(
                                target: LOG_TARGET,
                                "manage memory skipped: session switch gate closed: {error:?}"
                            );
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: "Session is being switched, please retry."
                                        .to_string(),
                                    is_error: true,
                                })
                                .await;
                        }
                    }
                    continue;
                }
                PendingCommand::ResumeSession { id } => {
                    use sdk::SessionResumeFailureKind;
                    match crate::application::client::resume_session_to_backing(&id, &wiring).await {
                        Ok((restore, resumed_id)) => {
                            // Update LOCAL loop variables only. The shared
                            // backing (`active_summary_arc` / `frozen_chats`)
                            // was already updated atomically by the
                            // SessionProjectionParticipant inside the
                            // exclusive gate during `resume_prepared` — we do
                            // NOT double-write them here.
                            chain = restore.active_chain;
                            active_summary = restore.active_summary.clone();
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumed {
                                    messages: chain.messages_flat(),
                                    session_id: resumed_id.clone(),
                                    created_at: chrono::DateTime::parse_from_rfc3339(
                                        &restore.created_at,
                                    )
                                    .map(|dt| dt.timestamp_millis() as u64)
                                    .unwrap_or(0),
                                })
                                .await;
                        }
                        Err(crate::application::client::ResumeError::Load(e)) => {
                            use context::session::SessionLoadError;
                            let (kind, message) = match &e {
                                SessionLoadError::NotFound { .. } => (
                                    SessionResumeFailureKind::NotFound,
                                    format!("Session {id} 不存在，可用 `/sessions` 查看可用会话"),
                                ),
                                SessionLoadError::Corrupt {
                                    parse_err,
                                    corrupt_path,
                                    ..
                                } => (
                                    SessionResumeFailureKind::Corrupt,
                                    format!(
                                        "Session {id} 损坏（{parse_err}），原文件已转存到 {}",
                                        corrupt_path.display()
                                    ),
                                ),
                                SessionLoadError::Io { source, .. } => (
                                    SessionResumeFailureKind::Io,
                                    format!("读取 session {id} 失败: {source}"),
                                ),
                            };
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumeFailed {
                                    kind,
                                    id: id.clone(),
                                    message,
                                })
                                .await;
                        }
                        Err(crate::application::client::ResumeError::Coordinator(
                            e,
                        )) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumeFailed {
                                    kind: SessionResumeFailureKind::Io,
                                    id: id.clone(),
                                    message: format!("resume_prepared failed: {e}"),
                                })
                                .await;
                        }
                    }
                    continue;
                }
                PendingCommand::RunReflection => match run_reflection_on_demand().await {
                    Ok(view) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::ReflectionResult {
                                output: Box::new(view),
                            })
                            .await;
                        continue;
                    }
                    Err(e) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("Reflection failed: {e}"),
                                is_error: true,
                            })
                            .await;
                        continue;
                    }
                },
                PendingCommand::ApplyReflection { output } => {
                    match apply_reflection_on_demand(output).await {
                        Ok(msg) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: msg,
                                    is_error: false,
                                })
                                .await;
                            continue;
                        }
                        Err(e) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Apply reflection failed: {e}"),
                                    is_error: true,
                                })
                                .await;
                            continue;
                        }
                    }
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
                &mut chain,
                &next_segment,
                &task_store,
                task_access.as_ref(),
                true,
            )
            .await;
            if let Some(command) = gate.pending_command {
                IdleResult::CommandRequested(command)
            } else if gate.appended_user_messages > 0 {
                IdleResult::Resumed(next_segment)
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
                &mut chain,
                &next_segment,
                &task_store,
                task_access.as_ref(),
                true,
            )
            .await;
            if let Some(command) = gate.pending_command {
                IdleResult::CommandRequested(command)
            } else if gate.appended_user_messages > 0 {
                IdleResult::Resumed(next_segment)
            } else {
                continue;
            }
        } else {
            idle_until_resume_or_shutdown(
                &input_events,
                &sink,
                &mut pending_input,
                &mut chain,
                &task_store,
                task_access.as_ref(),
            )
            .await
        };

        let segment_id = match idle_result {
            IdleResult::Shutdown => break 'session,
            IdleResult::CommandRequested(command) => handle_pending_command!(command),
            IdleResult::Resumed(next_segment) => next_segment,
        };

        turn_count += 1;
        let turn_context = RuntimeTurnContext::new(chat_id.clone(), ChatTurnId::new_v7());
        sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
            .await;
        let rollback_chain = chain.clone();
        let rollback_frozen_chats = frozen_chats
            .lock()
            .map(|frozen| frozen.clone())
            .unwrap_or_default();
        let rollback_active_summary = active_summary.clone();
        let started_at = Instant::now();
        cwd = workspace.read().current_workspace_root();

        let text = chain
            .last_message()
            .map(|message| message.text_content())
            .unwrap_or_default();
        let observation = reasoning.observe(ReasoningSignal::UserMessage { text, turn_count });
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
            &mut chain,
            &language,
            &segment_id,
        )
        .await;

        let cancel = CancellationToken::new();
        let mut run = Run::new(RunSpec::main(), None);
        let run_id = run.id().clone();
        active_run.activate(run_id.clone(), cancel.clone());
        // Bind the Main Run BEFORE constructing the port so the port can borrow
        // the bound MemoryPort for the entire run lifetime. The shared admission
        // permit held by `_bound` blocks `wiring.resume_prepared` (exclusive)
        // until the run — including all Tools, Reflection, and Sub-runs — has
        // fully completed and `_bound` is dropped. `_bound` is declared before
        // `port`, so it is dropped afterwards (reverse declaration order),
        // keeping both the permit and the memory borrow valid through run_loop.
        //
        // `bound.config()` captures the committed ConfigSnapshot at bind time.
        let _bound = match wiring.bind_main_run().await {
            Ok(bound) => bound,
            Err(e) => {
                log::error!(target: LOG_TARGET, "bind_main_run failed (gate closed?): {e:?}");
                active_run.clear(&run_id);
                continue;
            }
        };
        // H3: use the bound config's MemoryConfig (reflects the target project
        // after a cross-project resume) instead of the stale bootstrap value
        // captured once in `ChatLoopContext`. Each Main Run reads its own
        // snapshot at bind time, so runtime resume → next run naturally
        // refreshes. The bound memory config drives memory injection,
        // precompact auto_compact, and interval reflection inside the port.
        let bound_memory_config = _bound.config().memory().clone();
        let mut port = MainRunPort {
            memory: _bound.memory(),
            sink: &sink,
            queue: &queue,
            input_events: &input_events,
            client: &client,
            registry: &registry,
            system_blocks: &system_blocks,
            system_prompt_text: &system_prompt_text,
            user_context: &user_context,
            chain: &mut chain,
            context_size,
            workspace: &workspace,
            session_id: &session_id,
            read_files: &read_files,
            session_reminders: &session_reminders,
            agent_runner: &agent_runner,
            tool_result_materializer: tool_result_materializer.as_ref(),
            allow_all,
            task_access: &task_access,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore: &agent_semaphore,
            hook_runner: &hook_runner,
            memory_config: &bound_memory_config,
            memory_source: &memory_source,
            language: &language,
            frozen_chats: &frozen_chats,
            active_summary: &mut active_summary,
            active_summary_arc: &active_summary_arc,
            reasoning: reasoning.as_ref(),
            save_chain: &save_chain,
            pending_input: &mut pending_input,
            deferred_user_inputs: &mut deferred_user_inputs,
            cancel: cancel.clone(),
            run_id: run_id.clone(),
            active_run: active_run.as_ref(),
            turn_count,
            segment_id: &segment_id,
            turn_context,
            rollback_chain,
            rollback_frozen_chats,
            rollback_active_summary,
            memory_cwd: memory_cwd.clone(),
            last_total_tokens: &mut last_total_tokens,
            task_reminder_state: &mut task_reminder_state,
            tool_identity: &tool_identity,
            started_at,
        };
        // Refresh context_size from the committed ConfigSnapshot for the next
        // iteration. This run already captured the ctx-provided value, matching
        // the original bind-then-update timing.
        context_size = _bound.config().context_size();
        if let Err(error) = run_loop(&mut run, &cancel, &mut port).await {
            log::error!(target: LOG_TARGET, "main shared run loop failed: {error}");
        }
        active_run.clear(&run_id);
    }
    chain
}
