use crate::business::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::business::agent::Agent;
use crate::business::chat::looping::compact::{auto_compact, manual_compact};
use crate::business::chat::looping::compact_outcome::apply_compact_outcome;
use crate::business::chat::looping::finalize::{
    finish_completed_loop, run_stop_hook_before_finish, stop_hook_block_limit_reached,
};
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::idle_lifecycle::{
    cancel_to_idle, current_cancel_token, execute_set_thinking, has_pending_user_turn,
    idle_until_resume_or_shutdown, is_new_user_turn_message, IdleResult,
};
use crate::business::chat::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::business::chat::looping::loop_helpers::{
    drain_and_apply_gate, is_user_cancelled_provider_error,
};
use crate::business::chat::looping::loop_phases::{
    build_api_messages, handle_turn_boundary_config,
};
use crate::business::chat::looping::memory_inject::build_memory_block;
use crate::business::chat::looping::post_batch::run_post_tool_batch;
use crate::business::chat::looping::reflection::{run_reflection, should_run_turn_reflection};
use crate::business::chat::looping::stall::StallDetector;
use crate::business::chat::looping::task_reminder::TaskReminderState;
use crate::business::chat::looping::tool_fuse::ToolCallFuse;
use crate::business::chat::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::business::chat::looping::{
    ChatEventSink, ChatLoopFsm, ChatLoopState, ChatLoopTransition, GateDecision, GateKind,
    InputEventDrainPort, PendingCommand, PendingInputBuffer, QueueDrainPort, RuntimeStreamEvent,
    RuntimeStreamHandler, RuntimeTurnContext,
};
use crate::business::reasoning_graph::GraphSignal;
use crate::LOG_TARGET;
use provider::api::StopReason;
use sdk::ids::{ChatId, ChatTurnId};
use share::message::Message;
use std::sync::Arc;

use super::loop_context::ChatLoopContext;

/// Background task: runs the agent loop and sends UI events via sink.
pub async fn process_chat_loop<S, Q, I>(
    ctx: ChatLoopContext<S, Q, I>,
) -> crate::business::session::ChatChain
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
        session_id,
        read_files,
        session_reminders,
        agent_runner,
        allow_all,
        cancel: cancel_slot,
        task_store,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        hook_runner,
        memory_config,
        language,
        frozen_chats,
        active_summary: active_summary_arc,
        reasoning_graph,
        build_switched_client,
        save_chain,
        run_reflection_on_demand,
        apply_reflection_on_demand,
        list_models,
        list_reminders,
        list_sessions,
    } = ctx;
    let mut client = client;
    let mut reasoning_graph = reasoning_graph;
    let hook_ui = HookUi::new(sink.clone());

    // workspace service 跨 chat 轮次持有：恢复 session 时已 restore 到正确位置，
    // 这里直接读取当前 root 作为 hook/日志的工作目录基准（忽略 seed cwd）。
    // 初始值用于 loop 前的 config_snapshot 注册；loop 内每 turn 头会重新读取，
    // 使 hook env 跟随中途的 worktree 切换。
    let mut cwd = project::api::WorkspaceRead::current_workspace_root(workspace.as_ref());
    // memory 读写绑定项目启动时的 cwd（init root），不受 worktree 切换影响。
    let memory_cwd = project::api::WorkspaceRead::initial_cwd(workspace.as_ref());
    log::info!(target: LOG_TARGET,
        "chat loop hook runner ready: workspace_root={} memory_root={} configured_events={}",
        cwd.display(),
        memory_cwd.display(),
        hook_runner.hook_count()
    );
    // `agent` 在每个回合内构造（见 loop 体顶部）：它持有「当前回合 token」的 clone，
    // 使 LLM 调用与 tool 执行观测到同一个 token。常驻 loop 中 token 会在 cancel 后重置，
    // 因此 agent 不能在 loop 外只构造一次（否则会固化已取消的旧 token）。

    // 取消回滚基线（per-turn）。常驻 loop 中已完成回合的消息累积在同一个
    // `messages` Vec 里，若用 loop 启动时的固定基线回滚，会把先前已完成回合一并删除
    // （数据丢失）。因此每个回合在「本回合用户消息已入 messages、但本回合 assistant/tool
    // 输出尚未产生」处重新捕获基线，使 cancel 只回滚当前回合内容、保留先前已完成回合。
    //
    // 捕获时机（与重构前 per-`chat()` 语义对齐：彼时 `messages` 已含本回合用户消息，
    // cancel 保留用户消息、只回滚 partial assistant/tool 输出）：
    // - 回合开始（loop 顶）先按当时 `messages.len()` 设基线：resume / ContinueNextTurn
    //   路径在上一轮 `continue` 前已 append 本回合用户消息，故此处已计入；首回合若为预置
    //   seed 同样计入。覆盖回合起点 `cancel.is_cancelled()` 早退场景。
    // - BeforeLlm 门禁后再次刷新：覆盖「首回合空 seed 经门禁 append 用户消息」「同回合
    //   ContinueNextTurn 经门禁 append」等用户消息在本回合迭代内才入 messages 的情形。
    //
    // 声明为未初始化：每个回合在 loop 顶（见下）无条件赋值后才会被读，避免「初始值从未
    // 被读取」的 dead-store 告警。
    let mut turn_rollback_baseline: usize;
    let mut active_summary: Option<String> = None;
    let mut last_api_input_tokens: u64 = 0;
    let mut last_api_output_tokens: u64 = 0;
    let mut cached_tokens: Option<u64> = None;
    let mut reasoning_tokens: Option<u64> = None;
    // per-user-turn：每个新 USER 回合开始时重置（见 loop 体内的回合边界重置），
    // 使 `DoneWithDuration` 的 duration 反映本回合时长而非会话总时长（#390 A1）。
    let mut turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;
    let mut segment_id = String::new();
    let mut task_reminder_state = TaskReminderState::new();
    let mut stall_detector = StallDetector::new();
    let mut stop_hook_block_count: usize = 0;
    let mut pending_input = PendingInputBuffer::default();
    // busy 阶段（LLM 调用中）排队的用户输入。
    // idle 门开启时 drain 进 pending_input → apply_gate。
    let mut loop_fsm = ChatLoopFsm::default();
    let tool_identity = crate::business::chat::looping::tool_identity::ToolIdentityRegistry::new();
    let mut tool_call_fuse = ToolCallFuse::new();
    let chat_id = ChatId::new_v7();
    // 将 chat_id 同步到日志 context，影响 tool/audit/hook 等共享 sink 的 chat 字段
    logging::context::set_current_chat_id(chat_id.to_string());
    // 初始化配置变更快照注册表（turn 边界轮询用）
    let mut config_snapshot =
        crate::business::chat::looping::config_reload::init_snapshot_registry(&cwd);
    // #672：仅首次 loop-top 迭代无条件等待 pending_input（runtime 启动 / resume /
    // load session 后永远等用户输入，不管 messages 末尾）。后续迭代的 idle 由
    // completion arm / cancel arm 内部的 idle_until_resume_or_shutdown 负责，
    // 避免 completion arm Resumed 后 continue 回 loop-top 的 double-idle 死锁。
    let mut first_loop_top = true;

    // 处理 idle 期收到的 `PendingCommand`。
    // 单一真相：消除 #628 之前 loop_runner.rs 中 6 处复制粘贴的
    // IdleResult::CommandRequested(cmd) => match cmd { ... } match 臂。
    // #628 根因：9 个纯查询 / 动作命令变体漏了 continue，处理完后掉进
    // execute_tool_round 跑一轮无新输入的幽灵 LLM turn。本 macro 统一语义：
    // 所有 12 个变体处理完一律 continue 回 loop 顶部（回 idle 等下一条输入）。
    //
    // 参数：
    // - $cmd：待处理的 PendingCommand 值
    // - $needs_resume：bool 表达式。为 true 时（区间 4：turn 完成 → Idle 后
    //   收到命令），所有命令分支在 continue 前先把 FSM 从 Idle 切回 Running；
    //   其余 5 处 idle 上下文（cancel_to_idle 等）传 false
    macro_rules! handle_pending_command {
        ($cmd:expr, $needs_resume:expr) => {
            match $cmd {
                PendingCommand::Compact => {
                    if let Some(outcome) = manual_compact(
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        turn_count,
                        &chain.messages_flat(),
                        &system_prompt_text,
                        context_size,
                        &memory_config,
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
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    }
                    continue;
                }
                PendingCommand::SwitchModel { selection } => {
                    match (build_switched_client)(&selection).await {
                        Ok((new_client, result)) => {
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
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    }
                    continue;
                }
                PendingCommand::SetThinking { desired } => {
                    execute_set_thinking(&client, &sink, desired).await;
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    }
                    continue;
                }
                PendingCommand::InitProject { force } => {
                    let cwd_str = cwd.display().to_string();
                    let (text, is_error) = super::idle_commands::execute_init(&cwd_str, force);
                    let _ = sink
                        .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                        .await;
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    }
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
                        let (text, is_error) =
                            super::idle_commands::execute_session(&args, &session_id).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text,
                                is_error,
                            })
                            .await;
                    }
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    }
                    continue;
                }
                PendingCommand::ManageMemory { args } => {
                    let (text, is_error) = super::idle_commands::execute_memory(
                        &args,
                        &memory_cwd.display().to_string(),
                        &memory_config,
                    )
                    .await;
                    let _ = sink
                        .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                        .await;
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    }
                    continue;
                }
                PendingCommand::ResumeSession { id } => {
                    match crate::business::session::load_session(&id).await {
                        Ok(snapshot) => {
                            let restore =
                                crate::business::session::SessionRestore::from_session(&snapshot);
                            if restore.trimmed > 0 || restore.repaired > 0 {
                                log::info!(
                                    target: "aemeath:agent:runtime",
                                    "resume {}: trimmed={} repaired={}",
                                    id,
                                    restore.trimmed,
                                    restore.repaired
                                );
                            }
                            chain = restore.active_chain;
                            active_summary = restore.active_summary.clone();
                            if let Ok(mut guard) = active_summary_arc.lock() {
                                *guard = restore.active_summary;
                            }
                            if let Ok(mut guard) = frozen_chats.lock() {
                                *guard = restore.frozen_chats;
                            }
                            let _ = sink
                                .send_event(RuntimeStreamEvent::SessionResumed {
                                    messages: chain.messages_flat(),
                                    session_id: id.clone(),
                                    created_at: 0u64,
                                })
                                .await;
                            if restore.trimmed > 0 || restore.repaired > 0 {
                                log::info!(
                                    target: "aemeath:agent:runtime",
                                    "resume {}: trimmed={} repaired={}",
                                    id,
                                    restore.trimmed,
                                    restore.repaired
                                );
                            }
                        }
                        Err(e) => {
                            use crate::business::session::SessionLoadError;
                            use sdk::SessionResumeFailureKind;
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
                    }
                    if $needs_resume {
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
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
                        if $needs_resume {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        }
                        continue;
                    }
                    Err(e) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("Reflection failed: {e}"),
                                is_error: true,
                            })
                            .await;
                        if $needs_resume {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        }
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
                            if $needs_resume {
                                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                            }
                            continue;
                        }
                        Err(e) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Apply reflection failed: {e}"),
                                    is_error: true,
                                })
                                .await;
                            if $needs_resume {
                                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                            }
                            continue;
                        }
                    }
                }
                PendingCommand::ListModels => match list_models().await {
                    Ok(models) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::ModelList { models })
                            .await;
                        if $needs_resume {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        }
                        continue;
                    }
                    Err(e) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("List models failed: {e}"),
                                is_error: true,
                            })
                            .await;
                        if $needs_resume {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        }
                        continue;
                    }
                },
                PendingCommand::ListReminders => match list_reminders().await {
                    Ok(reminders) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::ReminderList { reminders })
                            .await;
                        if $needs_resume {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        }
                        continue;
                    }
                    Err(e) => {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText {
                                text: format!("List reminders failed: {e}"),
                                is_error: true,
                            })
                            .await;
                        if $needs_resume {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        }
                        continue;
                    }
                },
            }
        };
    }

    loop {
        // ── loop 顶部空闲门（Task 4，位于回合头之前）──
        // 若没有「待 assistant 响应的用户回合」（末条消息非 User），且 pending_input
        // 缓冲也为空（上游还没有新输入就绪），则在发出任何回合信号之前先 idle-wait：
        // 等到真正收到 UserMessage 后才开始本回合。
        //
        // **必须置于 `turn_count += 1` / `StartTurn` / `TurnChanged` /
        // `handle_turn_boundary_config` 之前**：否则空 seed 启动（TUI start-once 场景）
        // 会在尚无用户输入时就发出 `TurnChanged(1)`、跑 turn 边界配置，产生「回合 1 /
        // 处理中」的假信号；前置后，回合编号与回合信号只在真正有输入、回合真正开始时
        // 才推进（首个真实回合 = 1）。
        //
        // 这使 chat() 能以空 messages 或纯历史（末尾为 assistant）启动，loop 会阻塞
        // 等待第一条输入，而不是以空消息列表发起回合。
        //
        // 与回合完成后的空闲（completion arm）协作：completion arm 已 idle-wait 并把下
        // 一条 UserMessage append 进 messages 后再 continue；pending_input 非空 → 直接进入回合。
        //
        // FSM 注意：此处 FSM 处于 Running 态（loop 首轮默认 Running；后续轮经
        // `ResumeRunning` 回到 Running）。loop-top idle 是「回合前置等待」，不经过
        // Stopping→Idle→Done 路径；Shutdown 时直接从 Running 经 TryStop→StopSucceeded
        // 到 Done。
        //
        // `None` cancel_slot：前置等待不重置 cancel 槽——此时 loop 体的 `cancel` clone
        // 尚未读取（在本门之后才 `current_cancel_token`），重置会破坏首回合的外部 cancel。
        //
        // #672：runtime 启动 / resume / load session 后永远等待用户输入，
        // 不管 messages 末尾是什么角色。agent loop 内部多轮（tool call → 再调
        // LLM）由 FSM 的 ResumeRunning 状态转换驱动，不经过 idle 门。
        //
        // 首次 loop-top 迭代（startup / resume）：无条件等待 pending_input，
        // #672：runtime 启动 / resume / load session 后永远等待用户输入，
        // 不管 messages 末尾是什么角色。agent loop 内部多轮（tool call → 再调
        // LLM）由 FSM 的 ResumeRunning 状态转换驱动，不经过 idle 门。
        //
        // 后续迭代（first_loop_top=false）：completion arm / cancel arm 已通过
        // idle_until_resume_or_shutdown 处理了空闲等待，其 Resumed 路径会 continue
        // 回 loop-top。此时若 pending_input 为空但 messages 已含待答回合（user tail
        // 或 tool-result），不应再次 idle（否则 double-idle 死锁）。
        let should_idle = if first_loop_top {
            pending_input.is_empty()
        } else {
            !has_pending_user_turn(&chain.messages_flat()) && pending_input.is_empty()
        };
        first_loop_top = false;
        if should_idle {
            let idle_result = idle_until_resume_or_shutdown(
                &input_events,
                &sink,
                &mut pending_input,
                &mut chain,
                &task_store,
                None,
            )
            .await;
            match idle_result {
                IdleResult::Resumed(seg) => {
                    segment_id = seg;
                    // messages 已含新 UserMessage（由 idle 门内的 BeforeLlm gate 附加），
                    // pending_input 已清空。继续进入下方回合头：turn_count 推进、
                    // turn_rollback_baseline 在用户消息已入、assistant 未产生处捕获。
                }
                IdleResult::CommandRequested(cmd) => {
                    handle_pending_command!(cmd, false);
                }
                IdleResult::Shutdown => {
                    loop_fsm.transition(ChatLoopTransition::TryStop);
                    loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                    loop_fsm.assert_state(
                        ChatLoopState::Done,
                        "loop-top idle shuts down on channel close",
                    );
                    break;
                }
            }
        }

        turn_count += 1;
        let turn_id = ChatTurnId::new_v7();
        let turn_context = RuntimeTurnContext::new(chat_id.clone(), turn_id);
        loop_fsm.transition(ChatLoopTransition::StartTurn);
        sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
            .await;

        // 回合开始：以当前 messages 长度设取消回滚基线。此时本回合用户消息已由上一轮
        // 的 idle-resume / ContinueNextTurn gate（在 `continue` 之前）append 完成，或来自
        // 首回合 seed；先前已完成回合的消息均位于基线之内，cancel 不会触及。
        turn_rollback_baseline = chain.message_count();

        // 每 turn 头重新读取 workspace_root，使本 turn 的 hook env
        // （AEMEATH_PROJECT_DIR / CLAUDE_PROJECT_DIR）跟随中途的 worktree 切换。
        // 跟随本 turn 之前的 worktree 切换（EnterWorktree/ExitWorktree）。
        cwd = project::api::WorkspaceRead::current_workspace_root(workspace.as_ref());

        // ── 新 USER 回合边界：重置 per-user-turn 局部状态（#390 A1 跨回合泄漏修复）──
        // A1 把 loop 改为常驻（一个 loop 跨多个用户回合），导致原本 per-`chat()`（≈
        // per-user-turn）的 `stall_detector` / `turn_start` 退化为 per-session，跨回合泄漏：
        // - stall_detector：滑窗累积各回合 assistant 指纹，3 个独立回合各回一句相同短语
        //   （如 "Done."）会在第 3 回合误判「重复输出」停机（误报）。
        // - turn_start：从回合 2 起 `DoneWithDuration` 的 duration 变成会话总时长而非本回合。
        //
        // **判据（单一机制）**：仅当本回合是由一条「真正的新用户消息」开启时才重置——即
        // 末条消息 role=User、非 tool-result、且非 system-generated。这恰好覆盖所有新用户
        // 回合入口（loop 顶空闲门 resume、完成臂 idle resume、cancel idle resume、
        // ContinueNextTurn gate append），且**排除回合内的工具轮次再迭代**（工具结果消息
        // role 虽为 User 但 `has_tool_results()` 为真）与 stop-hook 阻断重试（注入的是
        // system-generated 用户消息）——因此单个回合内卡在循环仍能被 stall 检测捕获。
        if is_new_user_turn_message(chain.last_message()) {
            stall_detector = StallDetector::new();
            turn_start = std::time::Instant::now();
            // ReasoningGraph: 新用户消息触发阶段推断
            if let Some(graph) = reasoning_graph.as_mut() {
                let text = chain
                    .last_message()
                    .map(|m| m.text_content())
                    .unwrap_or_default();
                let prev = graph.current_node();
                if graph.transition(GraphSignal::UserMessage { text, turn_count }) {
                    sink.send_event(RuntimeStreamEvent::GraphPhaseChanged {
                        node: graph.current_node(),
                        effort: graph.current_effort(),
                        prev,
                    })
                    .await;
                }
            }
        }

        // ── 回合开始：从共享槽读取「当前回合 token」 ──
        // 锁仅用于 clone token 后立即释放（std::sync::Mutex，NEVER 跨 .await 持有）。
        // 外部 cancel_impl 锁同一槽对当前 token 调 cancel()；本回合的 LLM/tool 共用该 token。
        // cancel 处理后会 reset_cancel(&cancel_slot) 把槽换成新 token，下回合再从槽读取。
        let cancel = current_cancel_token(&cancel_slot);
        let agent = Agent {
            registry: &registry,
            ctx: tools::api::ToolExecutionContext {
                resources: tools::api::ToolResources {
                    agent_runner: agent_runner.clone(),
                    registry: Some(
                        registry.clone() as std::sync::Arc<dyn tools::api::ToolListProvider>
                    ),
                    memory_config: memory_config.clone(),
                    lang: language.clone(),
                    allow_all,
                },
                workspace: workspace.clone(),
                cancel: cancel.clone(),
                read_files: read_files.clone(),
                session_reminders: Some(session_reminders.clone()),
                plan_mode: None,
                max_tool_concurrency,
                max_agent_concurrency,
                agent_semaphore: agent_semaphore.clone(),
                progress_tx: None,
                parent_session_id: Some(session_id.clone()),
            },
        };

        // ── turn 边界：检测配置/指令/guidance 文件变更 ──
        handle_turn_boundary_config(
            &mut config_snapshot,
            turn_count,
            &sink,
            &mut chain,
            &language,
            &segment_id,
        )
        .await;

        // Refresh tool schemas each turn so dynamically registered MCP tools
        // are visible to the LLM once the background connector finishes.
        let tool_schemas = registry.schemas_for(&language);
        let tool_schema_tokens =
            crate::business::compact::estimate_tool_schemas_tokens(&tool_schemas);

        if cancel.is_cancelled() {
            // 回合起点即发现 token 已取消（外部在回合边界触发 cancel）：
            // 先看排队输入能否续跑；否则中止本回合、重置 token、回空闲。
            let outcome = drain_and_apply_gate(
                GateKind::BeforeFinish,
                &mut pending_input,
                &queue,
                &input_events,
                &sink,
                &mut chain,
                &segment_id,
                &task_store,
            )
            .await;
            if outcome.decision == GateDecision::ContinueNextTurn {
                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                continue;
            }
            let idle_result = cancel_to_idle(
                &sink,
                &input_events,
                &mut loop_fsm,
                &mut chain,
                &mut pending_input,
                &task_store,
                &cancel_slot,
                turn_rollback_baseline,
                &turn_context,
            )
            .await;
            match idle_result {
                IdleResult::Resumed(seg) => {
                    segment_id = seg;
                    continue;
                }
                IdleResult::CommandRequested(cmd) => {
                    handle_pending_command!(cmd, false);
                }
                IdleResult::Shutdown => {
                    loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                    loop_fsm.assert_state(
                        ChatLoopState::Done,
                        "cancel idle shuts down on channel close",
                    );
                    break;
                }
            }
        }

        loop_fsm.transition(ChatLoopTransition::Compact);
        // microcompact：规则驱动清理陈旧探索类 tool result（零 LLM 成本）。
        // 在 auto-compact 前执行，可能减少 token 足以跳过 LLM 摘要。
        // 保护最近 3 个 segment（大 loop），折叠更早的探索类 ToolResult。
        let mc_cleared = crate::business::compact::microcompact_chain(&mut chain);
        if mc_cleared > 0 {
            log::info!(target: crate::LOG_TARGET,
                "[microcompact] cleared {} stale exploratory tool results", mc_cleared);
            let _ = sink
                .send_event(RuntimeStreamEvent::MicrocompactDone {
                    messages: chain.messages_flat(),
                    cleared_count: mc_cleared,
                })
                .await;
        }
        // compact：发生时替换 messages 为 recent tail，summary 走 system。
        // resume 保护 + 产生时定型原则下，messages 产生后只在 compact 时被替换。
        if let Some(outcome) = auto_compact(
            &sink,
            &hook_ui,
            &hook_runner,
            turn_count,
            &chain.messages_flat(),
            &system_prompt_text,
            context_size,
            tool_schema_tokens,
            last_api_input_tokens,
            last_api_output_tokens,
            cached_tokens,
            reasoning_tokens,
            &memory_config,
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
        loop_fsm.transition(ChatLoopTransition::ResumeRunning);

        let gate = drain_and_apply_gate(
            GateKind::BeforeLlm,
            &mut pending_input,
            &queue,
            &input_events,
            &sink,
            &mut chain,
            &segment_id,
            &task_store,
        )
        .await;
        match gate.decision {
            GateDecision::Proceed | GateDecision::ContinueNextTurn => {
                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
            }
            GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop => {
                // before-llm 门禁收到取消 / /clear：中止本回合、重置 token、回空闲（不退 loop）。
                let idle_result = cancel_to_idle(
                    &sink,
                    &input_events,
                    &mut loop_fsm,
                    &mut chain,
                    &mut pending_input,
                    &task_store,
                    &cancel_slot,
                    turn_rollback_baseline,
                    &turn_context,
                )
                .await;
                match idle_result {
                    IdleResult::Resumed(seg) => {
                        segment_id = seg;
                        continue;
                    }
                    IdleResult::CommandRequested(cmd) => {
                        handle_pending_command!(cmd, true);
                    }
                    IdleResult::Shutdown => {
                        loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                        loop_fsm.assert_state(
                            ChatLoopState::Done,
                            "before-llm cancel idle shuts down on channel close",
                        );
                        break;
                    }
                }
            }
        }

        // BeforeLlm 门禁后刷新取消回滚基线：此处 messages 已含本回合用户消息
        // （首回合空 seed 经门禁 append，或同回合 ContinueNextTurn 经门禁 append），
        // 但本回合 LLM/tool 输出尚未产生。后续 assistant 消息（line ~440）、tool 结果
        // （line ~655）才是 cancel 应回滚的「本回合 partial 输出」。
        turn_rollback_baseline = chain.message_count();

        // Scan last assistant message for TaskCreate/TaskUpdate before building reminder
        task_reminder_state.update_from_messages(turn_count as u64, &chain.messages_flat());

        let messages_for_api: Vec<Message> = build_api_messages(
            &user_context,
            &language,
            &mut task_reminder_state,
            turn_count as u64,
            &task_store,
            &chain.messages_flat(),
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

        // memory 注入：每轮 LLM 调用前从 MemoryStore 取 top N 条注入为 system block。
        // 用 initial_cwd（非 worktree cwd）确保 memory 绑定项目身份。
        // cache_control = None：memory 内容可能随 reflection 新增条目而变，不缓存以隔离 cache 影响。
        let mut effective_system_blocks = system_blocks.clone();
        if memory_config.enabled && memory_config.inject_count > 0 {
            if let Some(block) = build_memory_block(&memory_cwd, memory_config.inject_count) {
                effective_system_blocks.push(block);
            }
        }

        // summary 注入 system_blocks（compact 后的摘要走 system 通道）
        if let Some(ref summary) = active_summary.clone() {
            effective_system_blocks.push(provider::api::SystemBlock {
                block_type: "text".to_string(),
                text: format!("<compact-summary>\n{summary}\n</compact-summary>"),
                cache_control: None,
            });
        }

        log_llm_input(
            &messages_for_api,
            chain.message_count(),
            &effective_system_blocks,
            &tool_schemas,
        );

        // ReasoningGraph: 按 graph 当前阶段调 effort（仅对支持 reasoning 的模型）
        if let Some(ref graph) = reasoning_graph {
            if graph.enabled() && client.is_reasoning() {
                let effort = graph
                    .current_effort()
                    .clamped_to(client.max_reasoning_level());
                client.set_reasoning_level(effort);
            }
        }

        let api_start = std::time::Instant::now();
        let response = {
            let progress_handle = handler.progress_handle();
            let stream_fut = client.stream_message(
                &effective_system_blocks,
                &messages_for_api,
                &tool_schemas,
                &mut handler,
                &cancel,
            );
            let waiting_sink = sink.clone();
            let waiting_context = turn_context.clone();
            let request_started_at = tokio::time::Instant::now();
            let waiting_task = tokio::spawn(async move {
                let mut next_waiting_at = request_started_at + std::time::Duration::from_secs(10);
                loop {
                    tokio::time::sleep_until(next_waiting_at).await;
                    let elapsed_secs = request_started_at.elapsed().as_secs();
                    let snapshot = progress_handle.lock().unwrap().snapshot();
                    log::debug!(target: LOG_TARGET,
                        "runtime idle watcher fired: elapsed_secs={} phase={} visible_seen={} turn_id={}",
                        elapsed_secs,
                        snapshot.phase,
                        snapshot.first_visible_event_seen,
                        waiting_context.turn_id,
                    );
                    waiting_sink.try_send_event(RuntimeStreamEvent::ModelStreamWaiting {
                        context: waiting_context.clone(),
                        elapsed_secs,
                        phase: snapshot.phase.to_string(),
                    });
                    next_waiting_at += std::time::Duration::from_secs(10);
                }
            });
            tokio::pin!(stream_fut);
            let result = loop {
                tokio::select! {
                    response = &mut stream_fut => {
                        break response;
                    }
                    event = input_events.recv_next_input() => {
                        if let Some(event) = event {
                            match &event {
                                sdk::ChatInputEvent::UserMessage { id, text, .. } => {
                                    log::debug!(target: LOG_TARGET,
                                        "busy queued user message: session={} id={} text_preview={:?}",
                                        session_id, id, &text[..text.len().min(60)]);
                                    pending_input.push(event);
                                    let queued_snapshot: Vec<(sdk::InputId, Message)> = pending_input.user_message_snapshot();
                                    sink.send_event(RuntimeStreamEvent::UserMessagesQueued {
                                        queued: queued_snapshot,
                                    }).await;
                                }
                                sdk::ChatInputEvent::WithdrawAll => {
                                    let texts: Vec<String> = pending_input.drain_user_message_texts();
                                    let count = texts.len();
                                    log::debug!(target: LOG_TARGET,
                                        "busy withdraw all queued: session={} count={}",
                                        session_id, count);
                                    sink.send_event(RuntimeStreamEvent::UserMessagesWithdrawn {
                                        texts,
                                    }).await;
                                }
                                _ => {
                                    log::debug!(target: LOG_TARGET,
                                        "busy ignored input: session={}", session_id);
                                }
                            }
                        }
                    }
                }
            };
            waiting_task.abort();
            result
        };
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
                // output_tokens 已包含 reasoning_tokens，无需额外累加。
                // 优先使用 provider 返回的 total_tokens；缺失时回退到 input + output。
                let total_tokens = resp
                    .usage
                    .total_tokens
                    .map(|v| v as u64)
                    .unwrap_or(last_api_input_tokens + last_api_output_tokens);
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

                chain.push(resp.assistant_message.clone(), &segment_id);
                sink.send_event(RuntimeStreamEvent::TurnStarted {
                    messages: chain.messages_flat(),
                })
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
                        &mut chain,
                        &segment_id,
                        &task_store,
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
                        &cwd,
                    )
                    .await
                    {
                        stop_hook_block_count += 1;
                        if stop_hook_block_limit_reached(
                            stop_hook_block_count,
                            &sink,
                            &mut loop_fsm,
                            &stall_outcome,
                            &turn_context,
                            &task_store,
                        )
                        .await
                        {
                            break;
                        }
                        loop_fsm.transition(ChatLoopTransition::StopBlocked);
                        chain.push(
                            Message::system_generated_user(format!(
                                "<system-reminder>\n{feedback}\n</system-reminder>"
                            )),
                            &segment_id,
                        );
                        sink.send_event(RuntimeStreamEvent::StopHookBlocked {
                            messages: chain.messages_flat(),
                        })
                        .await;
                        stall_detector = StallDetector::new();
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        continue;
                    }
                    loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                    loop_fsm.assert_state(ChatLoopState::Done, "stall stop finalizes loop");
                    // #604：stall 完成退出也需发出 DoneWithDuration，否则 TUI spinner 永不停
                    finish_completed_loop(&stall_outcome, &sink, &turn_context, &task_store).await;
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
                    // ReasoningGraph: 无 tool call → 回 Idle
                    if let Some(graph) = reasoning_graph.as_mut() {
                        let prev = graph.current_node();
                        if graph.transition(GraphSignal::TextOnly) {
                            sink.send_event(RuntimeStreamEvent::GraphPhaseChanged {
                                node: graph.current_node(),
                                effort: graph.current_effort(),
                                prev,
                            })
                            .await;
                        }
                    }
                    loop_fsm.transition(ChatLoopTransition::TryStop);
                    let gate = drain_and_apply_gate(
                        GateKind::BeforeFinish,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut chain,
                        &segment_id,
                        &task_store,
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
                            &chain.messages_flat(),
                            &memory_cwd,
                            &client,
                            &system_prompt_text,
                            &language,
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
                    let stop_feedback = run_stop_hook_before_finish(
                        &outcome,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &session_id,
                        &language,
                        &cwd,
                    )
                    .await;
                    // [loop_debug] 关键分叉：Stop hook 放行 (None) 还是阻断 (Some)。
                    log::info!(target: crate::LOG_TARGET,
                        "[loop_debug] turn {} completed → stop_hook {}",
                        turn_count,
                        if stop_feedback.is_some() { "BLOCKED (will inject reminder + continue)" } else { "PASSED (→ Idle)" }
                    );
                    if let Some(feedback) = stop_feedback {
                        stop_hook_block_count += 1;
                        if stop_hook_block_limit_reached(
                            stop_hook_block_count,
                            &sink,
                            &mut loop_fsm,
                            &outcome,
                            &turn_context,
                            &task_store,
                        )
                        .await
                        {
                            break;
                        }
                        loop_fsm.transition(ChatLoopTransition::StopBlocked);
                        chain.push(
                            Message::system_generated_user(format!(
                                "<system-reminder>\n{feedback}\n</system-reminder>"
                            )),
                            &segment_id,
                        );
                        sink.send_event(RuntimeStreamEvent::StopHookBlocked {
                            messages: chain.messages_flat(),
                        })
                        .await;
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        loop_fsm
                            .assert_state(ChatLoopState::Running, "stop hook blocked resumes loop");
                        log::info!(target: crate::LOG_TARGET,
                            "[loop_debug] turn {} Stop hook blocked → loop resumed Running, NOT entering Idle",
                            turn_count
                        );
                        continue;
                    }
                    let gate = drain_and_apply_gate(
                        GateKind::BeforeFinish,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut chain,
                        &segment_id,
                        &task_store,
                    )
                    .await;
                    if gate.decision == GateDecision::ContinueNextTurn {
                        // [loop_debug] stop hook 放行后，gate 又收到新输入 → 继续跑而非进 Idle。
                        log::debug!(target: crate::LOG_TARGET,
                            "[loop_debug] post-stophook gate → ContinueNextTurn (appended={}) — 未进 Idle",
                            gate.appended_user_messages
                        );
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        continue;
                    }
                    // 回合完成、stop hook 放行：发出 Done，但不退出常驻 loop。
                    // 进入空闲态阻塞等待下一条输入；通道关闭才 shutdown 退出。
                    log::info!(target: crate::LOG_TARGET,
                        "[loop_debug] turn {} → entering Idle (等待用户输入)", turn_count);
                    finish_completed_loop(&outcome, &sink, &turn_context, &task_store).await;
                    // #636 D1: turn-level save —— 每轮 turn 完成立即落盘，避免进程被
                    // kill 时丢失已完成 turn（SIGTERM/SIGHUP handler 见 chat 启动入口）。
                    if let Err(e) = save_chain(&chain).await {
                        log::error!(
                            target: crate::LOG_TARGET,
                            "turn-level save_chain failed (turn {}): {} — 下次 exit 时仍会兜底 save",
                            turn_count,
                            e
                        );
                    }
                    loop_fsm.transition(ChatLoopTransition::Idle);
                    loop_fsm.assert_state(
                        ChatLoopState::Idle,
                        "completed loop idles after stop hooks pass",
                    );
                    let idle_result = idle_until_resume_or_shutdown(
                        &input_events,
                        &sink,
                        &mut pending_input,
                        &mut chain,
                        &task_store,
                        Some(&cancel_slot),
                    )
                    .await;
                    match idle_result {
                        IdleResult::Shutdown => {
                            loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                            loop_fsm.assert_state(
                                ChatLoopState::Done,
                                "idle loop shuts down on channel close",
                            );
                            break;
                        }
                        IdleResult::Resumed(seg) => {
                            segment_id = seg;
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                            continue;
                        }
                        IdleResult::CommandRequested(cmd) => {
                            handle_pending_command!(cmd, false);
                        }
                    }
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
                        &cancel,
                        &language,
                        &cwd,
                        &mut tool_call_fuse,
                    )
                    .await;

                    // ReasoningGraph: tool 执行完成 → 按结果推断阶段
                    if let Some(graph) = reasoning_graph.as_mut() {
                        // 构建 provider_id → (bash_command, declared_phase) 映射
                        let tool_meta: std::collections::HashMap<
                            &str,
                            (Option<&str>, Option<&str>),
                        > = tool_calls
                            .iter()
                            .map(|tc| {
                                let bash_cmd = if tc.name == "Bash" {
                                    tc.input.get("command").and_then(|v| v.as_str())
                                } else {
                                    None
                                };
                                let phase = tc.input.get("phase").and_then(|v| v.as_str());
                                (tc.provider_id.as_str(), (bash_cmd, phase))
                            })
                            .collect();
                        for result in &all_results {
                            let (bash_command, declared_phase) = tool_meta
                                .get(result.provider_id.as_str())
                                .copied()
                                .unwrap_or((None, None));
                            let prev = graph.current_node();
                            if graph.transition(GraphSignal::ToolCompleted {
                                tool_name: result.tool_name.clone(),
                                bash_command: bash_command.map(|s| s.to_string()),
                                is_error: result.outcome.is_error,
                                declared_phase: declared_phase.map(|s| s.to_string()),
                            }) {
                                sink.send_event(RuntimeStreamEvent::GraphPhaseChanged {
                                    node: graph.current_node(),
                                    effort: graph.current_effort(),
                                    prev,
                                })
                                .await;
                            }
                        }
                    }
                    // Build tool result message for API
                    let has_task_mutation = all_results
                        .iter()
                        .any(|r| super::events::is_task_store_mutation(&r.tool_name));
                    chain.push(tool_results_for_api(all_results, &session_id), &segment_id);
                    // Sync after tool execution
                    sink.send_event(RuntimeStreamEvent::PostToolExecutionSync {
                        messages: chain.messages_flat(),
                    })
                    .await;
                    // 若刚执行了 task store mutation 工具，推送 task snapshot（#642）
                    if has_task_mutation {
                        let snapshot = super::task_snapshot::build_task_snapshot(&task_store).await;
                        sink.send_event(RuntimeStreamEvent::TasksSnapshot {
                            tasks: Box::new(snapshot),
                        })
                        .await;
                    }
                    loop_fsm.transition(ChatLoopTransition::AwaitUser);
                    let gate = drain_and_apply_gate(
                        GateKind::AfterBlockingBoundary,
                        &mut pending_input,
                        &queue,
                        &input_events,
                        &sink,
                        &mut chain,
                        &segment_id,
                        &task_store,
                    )
                    .await;
                    if matches!(
                        gate.decision,
                        GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop
                    ) {
                        // tool 执行后门禁收到取消 / /clear：中止本回合、重置 token、回空闲。
                        let idle_result = cancel_to_idle(
                            &sink,
                            &input_events,
                            &mut loop_fsm,
                            &mut chain,
                            &mut pending_input,
                            &task_store,
                            &cancel_slot,
                            turn_rollback_baseline,
                            &turn_context,
                        )
                        .await;
                        match idle_result {
                            IdleResult::Resumed(seg) => {
                                segment_id = seg;
                                continue;
                            }
                            IdleResult::CommandRequested(cmd) => {
                                handle_pending_command!(cmd, false);
                            }
                            IdleResult::Shutdown => {
                                loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                                loop_fsm.assert_state(
                                    ChatLoopState::Done,
                                    "after-tool cancel idle shuts down on channel close",
                                );
                                break;
                            }
                        }
                    }
                    loop_fsm.transition(ChatLoopTransition::ResumeRunning);

                    run_post_tool_batch(
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &agent.ctx,
                        turn_count,
                        &cwd,
                    )
                    .await;
                }
            }
            Err(e) => {
                if is_user_cancelled_provider_error(&e)
                    // If user cancellation races with provider error reporting, classify
                    // generic abort/network errors as cancellation rather than API errors.
                    || cancel.is_cancelled()
                {
                    // LLM 调用被取消（provider 报 Cancelled，或本回合 token 已取消）：
                    // 中止本回合、重置 token、回空闲（常驻 loop 不退出）。
                    let idle_result = cancel_to_idle(
                        &sink,
                        &input_events,
                        &mut loop_fsm,
                        &mut chain,
                        &mut pending_input,
                        &task_store,
                        &cancel_slot,
                        turn_rollback_baseline,
                        &turn_context,
                    )
                    .await;
                    match idle_result {
                        IdleResult::Resumed(seg) => {
                            segment_id = seg;
                            continue;
                        }
                        IdleResult::CommandRequested(cmd) => {
                            handle_pending_command!(cmd, false);
                        }
                        IdleResult::Shutdown => {
                            loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                            loop_fsm.assert_state(
                                ChatLoopState::Done,
                                "api cancel idle shuts down on channel close",
                            );
                            break;
                        }
                    }
                }

                // #749：API 错误路径 NOT 再发 `RuntimeStreamEvent::Error`。
                // 其唯一去向是 TUI 渲染通道（convert.rs），会与下方 `ApiError`
                // 造成同一错误双渲染。gate/hook 不消费该 stream 事件，删除安全。
                let error_msg = e.to_string();
                let gate = drain_and_apply_gate(
                    GateKind::BeforeFinish,
                    &mut pending_input,
                    &queue,
                    &input_events,
                    &sink,
                    &mut chain,
                    &segment_id,
                    &task_store,
                )
                .await;
                if gate.decision == GateDecision::ContinueNextTurn {
                    loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                    continue;
                }
                // API error 不走 stop hook：先发 ApiError（携带错误供展示，保留
                // 已完成的 tool 轮次消息），再统一经 `finish_completed_loop` 发出
                // DoneWithDuration 作为 turn 结束信号，让 TUI 与正常完成路径一致地
                // 收口 processing 状态（#749）。
                sink.send_event(RuntimeStreamEvent::ApiError {
                    messages: chain.messages_flat(),
                    error: error_msg.clone(),
                })
                .await;
                // API error 也需落盘（与正常完成路径行 1220 对齐），否则
                // tokio::spawn 的 auto-save 可能被 TUI 端 drop race 掉，
                // 导致失败 turn 的消息（含已完成的 tool 轮次）丢失。
                if let Err(e) = save_chain(&chain).await {
                    log::error!(
                        target: crate::LOG_TARGET,
                        "api-error save_chain failed: {e} — 下次 exit 时仍会兜底 save"
                    );
                }
                // 统一 turn 结束信号：发出 DoneWithDuration（TUI 据此 stop_processing）。
                let outcome = AgentRunOutcome {
                    status: AgentRunStatus::ApiError(error_msg.clone()),
                    turns: turn_count,
                    duration: turn_start.elapsed(),
                    role: None,
                    model: client.model_name().to_string(),
                };
                log_agent_outcome(&outcome, &session_id);
                finish_completed_loop(&outcome, &sink, &turn_context, &task_store).await;
                // FSM：API 错误 turn 中止 → 经合法路径 TryStop → Idle 进入空闲态
                // （常驻 loop 不退出），等待下一条输入或通道关闭。替代原先从 Running
                // 直接 StopSucceeded → Done 的非法转换 + break 退出常驻 loop（#749）。
                loop_fsm.transition(ChatLoopTransition::TryStop);
                loop_fsm.transition(ChatLoopTransition::Idle);
                loop_fsm.assert_state(
                    ChatLoopState::Idle,
                    "api-error turn idles without stop hooks",
                );
                let idle_result = idle_until_resume_or_shutdown(
                    &input_events,
                    &sink,
                    &mut pending_input,
                    &mut chain,
                    &task_store,
                    Some(&cancel_slot),
                )
                .await;
                match idle_result {
                    IdleResult::Shutdown => {
                        loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                        loop_fsm.assert_state(
                            ChatLoopState::Done,
                            "api-error idle shuts down on channel close",
                        );
                        break;
                    }
                    IdleResult::Resumed(seg) => {
                        segment_id = seg;
                        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                        continue;
                    }
                    IdleResult::CommandRequested(cmd) => {
                        handle_pending_command!(cmd, false);
                    }
                }
            }
        }
    }
    // #604 维护契约：所有"turn 异常终止后退出 loop"的 break 路径，
    // MUST 在 break 前调用 `finish_completed_loop` 发出 `DoneWithDuration`，
    // 否则 TUI spinner 永不停。已覆盖：stop hook blocked 上限（stall + 正常完成）、
    // stall 放行退出。channel-close 类 break（用户取消/关闭）由 TUI 端处理。
    // API 错误路径（#749）改为回 idle 保持常驻 loop，收口时经 `finish_completed_loop`
    // 发出 `DoneWithDuration` 作为统一 turn 结束信号，仅在通道关闭时才 break shutdown。
    // 新增异常终止 break 路径时 MUST 遵守此契约并补充对应测试。
    chain
}
