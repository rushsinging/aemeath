use crate::business::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::business::agent::Agent;
use crate::business::chat::looping::apply_gate;
use crate::business::chat::looping::compact::{auto_compact, manual_compact, CompactOutcome};
use crate::business::chat::looping::finalize::{
    finalize_main_loop, finish_completed_loop, run_stop_hook_before_finish,
    stop_hook_block_limit_reached,
};
use crate::business::chat::looping::hook_ui::HookUi;
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
use crate::business::chat::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::business::chat::looping::{
    ChatEventSink, ChatLoopFsm, ChatLoopState, ChatLoopTransition, GateDecision, GateKind,
    InputEventDrainPort, PendingCommand, PendingInputBuffer, QueueDrainPort, RuntimeStreamEvent,
    RuntimeStreamHandler, RuntimeTurnContext,
};
use crate::business::reasoning_graph::{GraphSignal, ReasoningGraph};
use crate::LOG_TARGET;
use provider::api::StopReason;
use sdk::ids::{ChatId, ChatTurnId};
use share::message::Message;
use share::message::Role;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::api::ToolRegistry;

/// 模型切换构建器类型（#567）：接受 selection 字符串，async 返回
/// `(LlmClient, ModelSwitchResult)` 或 `String` 错误。
pub type SwitchClientFn = Arc<
    dyn Fn(
            &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<
                            (provider::api::LlmClient, sdk::ModelSwitchResult),
                            String,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// 单次 chat loop 的完整执行状态。
///
/// 由 `chat_impl()` 从 `RuntimeHandle` 构造，按值传入 `process_chat_loop()`，
/// 函数内解构消费。持有 session 级不变配置 + loop 专属可变状态（messages、cancel 等）。
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
    pub workspace: Arc<project::api::WorkspaceService>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<share::tool::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn tools::api::AgentRunner>>,
    pub allow_all: bool,
    /// 会话级取消令牌槽（常驻 actor 可重建）。
    ///
    /// loop 在每个回合开始时从该槽读取「当前 token」用于本回合的 LLM 调用、
    /// tool 执行；外部（`cancel_impl`）锁该槽对当前 token 调 `cancel()` 触发取消。
    /// 处理完一次取消后，loop 把槽**重置为新 token** 供下个回合，避免常驻 loop
    /// 中被取消的 token 永久污染后续回合。`std::sync::Mutex` —— NEVER 跨 `.await` 持有。
    pub cancel: Arc<std::sync::Mutex<CancellationToken>>,
    pub task_store: Arc<storage::api::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: hook::api::HookRunner,
    pub memory_config: share::config::MemoryConfig,
    pub language: String,
    /// Reasoning Graph 实例。`None` = 未启用（零行为变更）；`Some` = 启用，
    /// loop 在 4 个集成点调 transition 调节 effort。
    pub reasoning_graph: Option<ReasoningGraph>,
    /// Compact 时冻结的旧链（保留在 session 文件中供审计，resume 不加载）。
    pub frozen_chats: Arc<std::sync::Mutex<Vec<crate::business::session::ChatSegment>>>,
    /// 活跃链的 compact summary（走 system 通道注入）。
    pub active_summary: Arc<std::sync::Mutex<Option<String>>>,
    /// Resume 后首次 loop-top idle 门跳过 pending user turn（#503）。
    pub skip_first_pending_turn: bool,
    /// 模型切换构建器（#567）。由 core 层注入，避免 business 层反向依赖 core。
    /// idle 分支收到 `SwitchModel` 事件时调用，从 config 解析 selection 字符串，
    /// 返回新 `LlmClient` + `ModelSwitchResult`；解析失败返回 `String` 错误信息。
    pub build_switched_client: SwitchClientFn,
    /// 会话保存闭包（#567 S5）。由 core 层注入，idle 分支收到 `SaveSession` 时调用。
    pub save_session: Arc<
        dyn Fn() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), sdk::SdkError>> + Send>,
            > + Send
        + Sync,
    >,
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
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        mut messages,
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
        skip_first_pending_turn,
        build_switched_client,
        save_session,
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
    // #503：resume 后首次遇到 pending user turn 时跳过，改为 idle 等待新输入。
    // 消费一次后置 false，后续正常行为不受影响。
    let mut skip_pending = skip_first_pending_turn;
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
        // 一条 UserMessage append 进 messages 后再 continue；此时 has_pending_user_turn
        // 为 true，不会触发 double-wait。
        //
        // FSM 注意：此处 FSM 处于 Running 态（loop 首轮默认 Running；后续轮经
        // `ResumeRunning` 回到 Running）。loop-top idle 是「回合前置等待」，不经过
        // Stopping→Idle→Done 路径；Shutdown 时直接从 Running 经 TryStop→StopSucceeded
        // 到 Done。
        //
        // `None` cancel_slot：前置等待不重置 cancel 槽——此时 loop 体的 `cancel` clone
        // 尚未读取（在本门之后才 `current_cancel_token`），重置会破坏首回合的外部 cancel。
        //
        // #503：resume 后末条消息可能是等待 assistant 回复的 User 消息（纯文本或
        // tool_result）。此时 has_pending_user_turn 为 true，正常路径会自动发起 LLM
        // 请求恢复中断的对话。skip_pending 标志使首次遇到此情况时改为 idle 等待，
        // 让用户决定是否继续。idle 门内收到新 UserMessage 后 append 到 messages，
        // skip_pending 被消费为 false。
        let should_idle = (!has_pending_user_turn(&messages) && pending_input.is_empty())
            || (skip_pending && has_pending_user_turn(&messages));
        if should_idle {
            match idle_until_resume_or_shutdown(
                &input_events,
                &sink,
                &mut pending_input,
                &mut messages,
                &task_store,
                None,
            )
            .await
            {
                IdleResult::Resumed => {
                    // messages 已含新 UserMessage（由 idle 门内的 BeforeLlm gate 附加），
                    // pending_input 已清空。继续进入下方回合头：turn_count 推进、
                    // turn_rollback_baseline 在用户消息已入、assistant 未产生处捕获。
                    // #503：消费 skip 标志，后续回合恢复正常行为。
                    skip_pending = false;
                }
                IdleResult::CommandRequested(cmd) => match cmd {
                    PendingCommand::Compact => {
                        if let Some(outcome) = manual_compact(
                            &sink,
                            &hook_ui,
                            &hook_runner,
                            turn_count,
                            &messages,
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
                                &mut messages,
                                &frozen_chats,
                                &mut active_summary,
                                &active_summary_arc,
                            )
                            .await;
                        }
                        // compact 后回到 loop 顶重新检查 idle（无新用户消息则继续等待）
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
                        continue;
                    }
                    PendingCommand::SetThinking { desired } => {
                        execute_set_thinking(&client, &sink, desired).await;
                        continue;
                    }
                    PendingCommand::EstimateContext => {
                        execute_estimate_context(
                            &messages,
                            &system_prompt_text,
                            context_size,
                            &sink,
                        )
                        .await;
                        continue;
                    }
                    PendingCommand::QueryCost { args } => {
                        let (text, is_error) =
                            super::idle_commands::execute_cost(&args, &session_id).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::QueryStatus => {
                        let config = share::config::Config::default();
                        let cwd_str = cwd.display().to_string();
                        let (text, is_error) = super::idle_commands::execute_status(
                            &config,
                            &session_id,
                            &cwd_str,
                            client.model_name(),
                        );
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::QueryConfig { args } => {
                        let config = share::config::Config::default();
                        let (text, is_error) = super::idle_commands::execute_config(&args, &config);
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::QueryStats { args } => {
                        let config = share::config::Config::default();
                        let (text, is_error) =
                            super::idle_commands::execute_stats(&args, &session_id, &config).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::InitProject { force } => {
                        let cwd_str = cwd.display().to_string();
                        let (text, is_error) = super::idle_commands::execute_init(&cwd_str, force);
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::ManageSession { args } => {
                        let (text, is_error) =
                            super::idle_commands::execute_session(&args, &session_id).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::ManageMemory { args } => {
                        let (text, is_error) = super::idle_commands::execute_memory(
                            &args,
                            &memory_cwd.display().to_string(),
                        )
                        .await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::ResumeSession { id } => {
                        match crate::business::session::load_session(&id).await {
                            Ok(snapshot) => {
                                messages = snapshot.messages;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::SessionResumed {
                                        messages: messages.clone(),
                                        session_id: id.clone(),
                                        created_at: 0u64,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text: format!("Failed to resume session {}: {}", id, e),
                                        is_error: true,
                                    })
                                    .await;
                            }
                        }
                    }
                    PendingCommand::SaveSession => match (save_session)().await {
                        Ok(()) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Session saved: {}", session_id),
                                    is_error: false,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Failed to save session: {e}"),
                                    is_error: true,
                                })
                                .await;
                        }
                    },
                },
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
        turn_rollback_baseline = messages.len();

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
        if is_new_user_turn_message(messages.last()) {
            stall_detector = StallDetector::new();
            turn_start = std::time::Instant::now();
            // ReasoningGraph: 新用户消息触发阶段推断
            if let Some(graph) = reasoning_graph.as_mut() {
                let text = messages
                    .last()
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
            &mut messages,
            &language,
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
                &mut messages,
                &task_store,
            )
            .await;
            if outcome.decision == GateDecision::ContinueNextTurn {
                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                continue;
            }
            match cancel_to_idle(
                &sink,
                &input_events,
                &mut loop_fsm,
                &mut messages,
                &mut pending_input,
                &task_store,
                &cancel_slot,
                turn_rollback_baseline,
                &turn_context,
            )
            .await
            {
                IdleResult::Resumed => continue,
                IdleResult::CommandRequested(cmd) => match cmd {
                    PendingCommand::Compact => {
                        if let Some(outcome) = manual_compact(
                            &sink,
                            &hook_ui,
                            &hook_runner,
                            turn_count,
                            &messages,
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
                                &mut messages,
                                &frozen_chats,
                                &mut active_summary,
                                &active_summary_arc,
                            )
                            .await;
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
                        continue;
                    }
                    PendingCommand::SetThinking { desired } => {
                        execute_set_thinking(&client, &sink, desired).await;
                        continue;
                    }
                    PendingCommand::EstimateContext => {
                        execute_estimate_context(
                            &messages,
                            &system_prompt_text,
                            context_size,
                            &sink,
                        )
                        .await;
                        continue;
                    }
                    PendingCommand::QueryCost { args } => {
                        let (text, is_error) =
                            super::idle_commands::execute_cost(&args, &session_id).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::QueryStatus => {
                        let config = share::config::Config::default();
                        let cwd_str = cwd.display().to_string();
                        let (text, is_error) = super::idle_commands::execute_status(
                            &config,
                            &session_id,
                            &cwd_str,
                            client.model_name(),
                        );
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::QueryConfig { args } => {
                        let config = share::config::Config::default();
                        let (text, is_error) = super::idle_commands::execute_config(&args, &config);
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::QueryStats { args } => {
                        let config = share::config::Config::default();
                        let (text, is_error) =
                            super::idle_commands::execute_stats(&args, &session_id, &config).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::InitProject { force } => {
                        let cwd_str = cwd.display().to_string();
                        let (text, is_error) = super::idle_commands::execute_init(&cwd_str, force);
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::ManageSession { args } => {
                        let (text, is_error) =
                            super::idle_commands::execute_session(&args, &session_id).await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::ManageMemory { args } => {
                        let (text, is_error) = super::idle_commands::execute_memory(
                            &args,
                            &memory_cwd.display().to_string(),
                        )
                        .await;
                        let _ = sink
                            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
                            .await;
                    }
                    PendingCommand::ResumeSession { id } => {
                        match crate::business::session::load_session(&id).await {
                            Ok(snapshot) => {
                                messages = snapshot.messages;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::SessionResumed {
                                        messages: messages.clone(),
                                        session_id: id.clone(),
                                        created_at: 0u64,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text: format!("Failed to resume session {}: {}", id, e),
                                        is_error: true,
                                    })
                                    .await;
                            }
                        }
                    }
                    PendingCommand::SaveSession => match (save_session)().await {
                        Ok(()) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Session saved: {}", session_id),
                                    is_error: false,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text: format!("Failed to save session: {e}"),
                                    is_error: true,
                                })
                                .await;
                        }
                    },
                },
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
        let mc_cleared = crate::business::compact::microcompact_messages(&mut messages);
        if mc_cleared > 0 {
            log::info!(target: crate::LOG_TARGET,
                "[microcompact] cleared {} stale exploratory tool results", mc_cleared);
            let _ = sink
                .send_event(RuntimeStreamEvent::SystemMessage(format!(
                    "[microcompact: cleared {mc_cleared} old tool result(s)]"
                )))
                .await;
            // 同步到 TUI 镜像
            let _ = sink
                .send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                .await;
        }
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
                &mut messages,
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
            &mut messages,
            &task_store,
        )
        .await;
        match gate.decision {
            GateDecision::Proceed | GateDecision::ContinueNextTurn => {
                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
            }
            GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop => {
                // before-llm 门禁收到取消 / /clear：中止本回合、重置 token、回空闲（不退 loop）。
                match cancel_to_idle(
                    &sink,
                    &input_events,
                    &mut loop_fsm,
                    &mut messages,
                    &mut pending_input,
                    &task_store,
                    &cancel_slot,
                    turn_rollback_baseline,
                    &turn_context,
                )
                .await
                {
                    IdleResult::Resumed => continue,
                    IdleResult::CommandRequested(cmd) => match cmd {
                        PendingCommand::Compact => {
                            if let Some(outcome) = manual_compact(
                                &sink,
                                &hook_ui,
                                &hook_runner,
                                turn_count,
                                &messages,
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
                                    &mut messages,
                                    &frozen_chats,
                                    &mut active_summary,
                                    &active_summary_arc,
                                )
                                .await;
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
                            continue;
                        }
                        PendingCommand::SetThinking { desired } => {
                            execute_set_thinking(&client, &sink, desired).await;
                            continue;
                        }
                        PendingCommand::EstimateContext => {
                            execute_estimate_context(
                                &messages,
                                &system_prompt_text,
                                context_size,
                                &sink,
                            )
                            .await;
                            continue;
                        }
                        PendingCommand::QueryCost { args } => {
                            let (text, is_error) =
                                super::idle_commands::execute_cost(&args, &session_id).await;
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::QueryStatus => {
                            let config = share::config::Config::default();
                            let cwd_str = cwd.display().to_string();
                            let (text, is_error) = super::idle_commands::execute_status(
                                &config,
                                &session_id,
                                &cwd_str,
                                client.model_name(),
                            );
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::QueryConfig { args } => {
                            let config = share::config::Config::default();
                            let (text, is_error) =
                                super::idle_commands::execute_config(&args, &config);
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::QueryStats { args } => {
                            let config = share::config::Config::default();
                            let (text, is_error) =
                                super::idle_commands::execute_stats(&args, &session_id, &config)
                                    .await;
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::InitProject { force } => {
                            let cwd_str = cwd.display().to_string();
                            let (text, is_error) =
                                super::idle_commands::execute_init(&cwd_str, force);
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::ManageSession { args } => {
                            let (text, is_error) =
                                super::idle_commands::execute_session(&args, &session_id).await;
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::ManageMemory { args } => {
                            let (text, is_error) = super::idle_commands::execute_memory(
                                &args,
                                &memory_cwd.display().to_string(),
                            )
                            .await;
                            let _ = sink
                                .send_event(RuntimeStreamEvent::CommandResultText {
                                    text,
                                    is_error,
                                })
                                .await;
                        }
                        PendingCommand::ResumeSession { id } => {
                            match crate::business::session::load_session(&id).await {
                                Ok(snapshot) => {
                                    messages = snapshot.messages;
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::SessionResumed {
                                            messages: messages.clone(),
                                            session_id: id.clone(),
                                            created_at: 0u64,
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text: format!("Failed to resume session {}: {}", id, e),
                                            is_error: true,
                                        })
                                        .await;
                                }
                            }
                        }
                        PendingCommand::SaveSession => match (save_session)().await {
                            Ok(()) => {
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text: format!("Session saved: {}", session_id),
                                        is_error: false,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text: format!("Failed to save session: {e}"),
                                        is_error: true,
                                    })
                                    .await;
                            }
                        },
                    },
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
        turn_rollback_baseline = messages.len();

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
            messages.len(),
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
                        &mut messages,
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
                            &messages,
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
                    if let Some(outcome) = run_stop_hook_before_finish(
                        &outcome,
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
                        &task_store,
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
                    match idle_until_resume_or_shutdown(
                        &input_events,
                        &sink,
                        &mut pending_input,
                        &mut messages,
                        &task_store,
                        Some(&cancel_slot),
                    )
                    .await
                    {
                        IdleResult::Shutdown => {
                            loop_fsm.transition(ChatLoopTransition::StopSucceeded);
                            loop_fsm.assert_state(
                                ChatLoopState::Done,
                                "idle loop shuts down on channel close",
                            );
                            break;
                        }
                        IdleResult::Resumed => {
                            loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                            continue;
                        }
                        IdleResult::CommandRequested(cmd) => match cmd {
                            PendingCommand::Compact => {
                                if let Some(outcome) = manual_compact(
                                    &sink,
                                    &hook_ui,
                                    &hook_runner,
                                    turn_count,
                                    &messages,
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
                                        &mut messages,
                                        &frozen_chats,
                                        &mut active_summary,
                                        &active_summary_arc,
                                    )
                                    .await;
                                }
                                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                                continue;
                            }
                            PendingCommand::SwitchModel { selection } => {
                                match (build_switched_client)(&selection).await {
                                    Ok((new_client, result)) => {
                                        client = Arc::new(new_client);
                                        context_size = result.context_window;
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::ModelSwitched {
                                                result,
                                            })
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
                                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                                continue;
                            }
                            PendingCommand::SetThinking { desired } => {
                                execute_set_thinking(&client, &sink, desired).await;
                                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                                continue;
                            }
                            PendingCommand::EstimateContext => {
                                execute_estimate_context(
                                    &messages,
                                    &system_prompt_text,
                                    context_size,
                                    &sink,
                                )
                                .await;
                                loop_fsm.transition(ChatLoopTransition::ResumeRunning);
                                continue;
                            }
                            PendingCommand::QueryCost { args } => {
                                let (text, is_error) =
                                    super::idle_commands::execute_cost(&args, &session_id).await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::QueryStatus => {
                                let config = share::config::Config::default();
                                let cwd_str = cwd.display().to_string();
                                let (text, is_error) = super::idle_commands::execute_status(
                                    &config,
                                    &session_id,
                                    &cwd_str,
                                    client.model_name(),
                                );
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::QueryConfig { args } => {
                                let config = share::config::Config::default();
                                let (text, is_error) =
                                    super::idle_commands::execute_config(&args, &config);
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::QueryStats { args } => {
                                let config = share::config::Config::default();
                                let (text, is_error) = super::idle_commands::execute_stats(
                                    &args,
                                    &session_id,
                                    &config,
                                )
                                .await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::InitProject { force } => {
                                let cwd_str = cwd.display().to_string();
                                let (text, is_error) =
                                    super::idle_commands::execute_init(&cwd_str, force);
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::ManageSession { args } => {
                                let (text, is_error) =
                                    super::idle_commands::execute_session(&args, &session_id).await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::ManageMemory { args } => {
                                let (text, is_error) = super::idle_commands::execute_memory(
                                    &args,
                                    &memory_cwd.display().to_string(),
                                )
                                .await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::ResumeSession { id } => {
                                match crate::business::session::load_session(&id).await {
                                    Ok(snapshot) => {
                                        messages = snapshot.messages;
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::SessionResumed {
                                                messages: messages.clone(),
                                                session_id: id.clone(),
                                                created_at: 0u64,
                                            })
                                            .await;
                                    }
                                    Err(e) => {
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::CommandResultText {
                                                text: format!(
                                                    "Failed to resume session {}: {}",
                                                    id, e
                                                ),
                                                is_error: true,
                                            })
                                            .await;
                                    }
                                }
                            }
                            PendingCommand::SaveSession => match (save_session)().await {
                                Ok(()) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text: format!("Session saved: {}", session_id),
                                            is_error: false,
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text: format!("Failed to save session: {e}"),
                                            is_error: true,
                                        })
                                        .await;
                                }
                            },
                        },
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
                        max_agent_concurrency,
                        &cancel,
                        &language,
                        &cwd,
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
                        &task_store,
                    )
                    .await;
                    if matches!(
                        gate.decision,
                        GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop
                    ) {
                        // tool 执行后门禁收到取消 / /clear：中止本回合、重置 token、回空闲。
                        match cancel_to_idle(
                            &sink,
                            &input_events,
                            &mut loop_fsm,
                            &mut messages,
                            &mut pending_input,
                            &task_store,
                            &cancel_slot,
                            turn_rollback_baseline,
                            &turn_context,
                        )
                        .await
                        {
                            IdleResult::Resumed => continue,
                            IdleResult::CommandRequested(cmd) => match cmd {
                                PendingCommand::Compact => {
                                    if let Some(outcome) = manual_compact(
                                        &sink,
                                        &hook_ui,
                                        &hook_runner,
                                        turn_count,
                                        &messages,
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
                                            &mut messages,
                                            &frozen_chats,
                                            &mut active_summary,
                                            &active_summary_arc,
                                        )
                                        .await;
                                    }
                                    continue;
                                }
                                PendingCommand::SwitchModel { selection } => {
                                    match (build_switched_client)(&selection).await {
                                        Ok((new_client, result)) => {
                                            client = Arc::new(new_client);
                                            context_size = result.context_window;
                                            let _ = sink
                                                .send_event(RuntimeStreamEvent::ModelSwitched {
                                                    result,
                                                })
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
                                    execute_set_thinking(&client, &sink, desired).await;
                                    continue;
                                }
                                PendingCommand::EstimateContext => {
                                    execute_estimate_context(
                                        &messages,
                                        &system_prompt_text,
                                        context_size,
                                        &sink,
                                    )
                                    .await;
                                    continue;
                                }
                                PendingCommand::QueryCost { args } => {
                                    let (text, is_error) =
                                        super::idle_commands::execute_cost(&args, &session_id)
                                            .await;
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::QueryStatus => {
                                    let config = share::config::Config::default();
                                    let cwd_str = cwd.display().to_string();
                                    let (text, is_error) = super::idle_commands::execute_status(
                                        &config,
                                        &session_id,
                                        &cwd_str,
                                        client.model_name(),
                                    );
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::QueryConfig { args } => {
                                    let config = share::config::Config::default();
                                    let (text, is_error) =
                                        super::idle_commands::execute_config(&args, &config);
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::QueryStats { args } => {
                                    let config = share::config::Config::default();
                                    let (text, is_error) = super::idle_commands::execute_stats(
                                        &args,
                                        &session_id,
                                        &config,
                                    )
                                    .await;
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::InitProject { force } => {
                                    let cwd_str = cwd.display().to_string();
                                    let (text, is_error) =
                                        super::idle_commands::execute_init(&cwd_str, force);
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::ManageSession { args } => {
                                    let (text, is_error) =
                                        super::idle_commands::execute_session(&args, &session_id)
                                            .await;
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::ManageMemory { args } => {
                                    let (text, is_error) = super::idle_commands::execute_memory(
                                        &args,
                                        &memory_cwd.display().to_string(),
                                    )
                                    .await;
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text,
                                            is_error,
                                        })
                                        .await;
                                }
                                PendingCommand::ResumeSession { id } => {
                                    match crate::business::session::load_session(&id).await {
                                        Ok(snapshot) => {
                                            messages = snapshot.messages;
                                            let _ = sink
                                                .send_event(RuntimeStreamEvent::SessionResumed {
                                                    messages: messages.clone(),
                                                    session_id: id.clone(),
                                                    created_at: 0u64,
                                                })
                                                .await;
                                        }
                                        Err(e) => {
                                            let _ = sink
                                                .send_event(RuntimeStreamEvent::CommandResultText {
                                                    text: format!(
                                                        "Failed to resume session {}: {}",
                                                        id, e
                                                    ),
                                                    is_error: true,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                PendingCommand::SaveSession => match (save_session)().await {
                                    Ok(()) => {
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::CommandResultText {
                                                text: format!("Session saved: {}", session_id),
                                                is_error: false,
                                            })
                                            .await;
                                    }
                                    Err(e) => {
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::CommandResultText {
                                                text: format!("Failed to save session: {e}"),
                                                is_error: true,
                                            })
                                            .await;
                                    }
                                },
                            },
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
                    match cancel_to_idle(
                        &sink,
                        &input_events,
                        &mut loop_fsm,
                        &mut messages,
                        &mut pending_input,
                        &task_store,
                        &cancel_slot,
                        turn_rollback_baseline,
                        &turn_context,
                    )
                    .await
                    {
                        IdleResult::Resumed => continue,
                        IdleResult::CommandRequested(cmd) => match cmd {
                            PendingCommand::Compact => {
                                if let Some(outcome) = manual_compact(
                                    &sink,
                                    &hook_ui,
                                    &hook_runner,
                                    turn_count,
                                    &messages,
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
                                        &mut messages,
                                        &frozen_chats,
                                        &mut active_summary,
                                        &active_summary_arc,
                                    )
                                    .await;
                                }
                                continue;
                            }
                            PendingCommand::SwitchModel { selection } => {
                                match (build_switched_client)(&selection).await {
                                    Ok((new_client, result)) => {
                                        client = Arc::new(new_client);
                                        context_size = result.context_window;
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::ModelSwitched {
                                                result,
                                            })
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
                                execute_set_thinking(&client, &sink, desired).await;
                                continue;
                            }
                            PendingCommand::EstimateContext => {
                                execute_estimate_context(
                                    &messages,
                                    &system_prompt_text,
                                    context_size,
                                    &sink,
                                )
                                .await;
                                continue;
                            }
                            PendingCommand::QueryCost { args } => {
                                let (text, is_error) =
                                    super::idle_commands::execute_cost(&args, &session_id).await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::QueryStatus => {
                                let config = share::config::Config::default();
                                let cwd_str = cwd.display().to_string();
                                let (text, is_error) = super::idle_commands::execute_status(
                                    &config,
                                    &session_id,
                                    &cwd_str,
                                    client.model_name(),
                                );
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::QueryConfig { args } => {
                                let config = share::config::Config::default();
                                let (text, is_error) =
                                    super::idle_commands::execute_config(&args, &config);
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::QueryStats { args } => {
                                let config = share::config::Config::default();
                                let (text, is_error) = super::idle_commands::execute_stats(
                                    &args,
                                    &session_id,
                                    &config,
                                )
                                .await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::InitProject { force } => {
                                let cwd_str = cwd.display().to_string();
                                let (text, is_error) =
                                    super::idle_commands::execute_init(&cwd_str, force);
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::ManageSession { args } => {
                                let (text, is_error) =
                                    super::idle_commands::execute_session(&args, &session_id).await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::ManageMemory { args } => {
                                let (text, is_error) = super::idle_commands::execute_memory(
                                    &args,
                                    &memory_cwd.display().to_string(),
                                )
                                .await;
                                let _ = sink
                                    .send_event(RuntimeStreamEvent::CommandResultText {
                                        text,
                                        is_error,
                                    })
                                    .await;
                            }
                            PendingCommand::ResumeSession { id } => {
                                match crate::business::session::load_session(&id).await {
                                    Ok(snapshot) => {
                                        messages = snapshot.messages;
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::SessionResumed {
                                                messages: messages.clone(),
                                                session_id: id.clone(),
                                                created_at: 0u64,
                                            })
                                            .await;
                                    }
                                    Err(e) => {
                                        let _ = sink
                                            .send_event(RuntimeStreamEvent::CommandResultText {
                                                text: format!(
                                                    "Failed to resume session {}: {}",
                                                    id, e
                                                ),
                                                is_error: true,
                                            })
                                            .await;
                                    }
                                }
                            }
                            PendingCommand::SaveSession => match (save_session)().await {
                                Ok(()) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text: format!("Session saved: {}", session_id),
                                            is_error: false,
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = sink
                                        .send_event(RuntimeStreamEvent::CommandResultText {
                                            text: format!("Failed to save session: {e}"),
                                            is_error: true,
                                        })
                                        .await;
                                }
                            },
                        },
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
                    &task_store,
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
                    &cwd,
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

/// idle 分支执行 `/think`：读当前 reasoning level，按 desired 设置新 level，
/// 发 `ThinkingChanged` + `SystemMessage`。
async fn execute_set_thinking<S>(client: &provider::api::LlmClient, sink: &S, desired: Option<bool>)
where
    S: ChatEventSink,
{
    use provider::api::ReasoningLevel;
    let current = client.current_reasoning_level();
    let new_state = desired.unwrap_or(matches!(current, ReasoningLevel::Off));
    let level = if new_state {
        ReasoningLevel::Medium
    } else {
        ReasoningLevel::Off
    };
    client.set_reasoning_level(level);
    let label = if new_state { "ON" } else { "OFF" };
    let _ = sink
        .send_event(RuntimeStreamEvent::ThinkingChanged { enabled: new_state })
        .await;
    let _ = sink
        .send_event(RuntimeStreamEvent::SystemMessage(format!(
            "[thinking mode: {}]",
            label
        )))
        .await;
}

/// idle 分支执行 `/context`：用 loop 内部 messages + system_prompt 估算 token 占用，
/// 发 `ContextEstimated` 事件（TUI 据此显示）。
async fn execute_estimate_context<S>(
    messages: &[share::message::Message],
    system_prompt_text: &str,
    context_size: usize,
    sink: &S,
) where
    S: ChatEventSink,
{
    let estimated_tokens = crate::business::compact::estimate_messages_tokens(messages)
        + crate::business::compact::estimate_tokens(system_prompt_text);
    let system_tokens = crate::business::compact::estimate_tokens(system_prompt_text);
    let usage_percentage = if context_size > 0 {
        estimated_tokens as f64 * 100.0 / context_size as f64
    } else {
        0.0
    };
    let estimate = sdk::ContextEstimate {
        estimated_tokens,
        system_tokens,
        context_size,
        usage_percentage,
    };
    let _ = sink
        .send_event(RuntimeStreamEvent::ContextEstimated {
            estimate,
            message_count: messages.len(),
        })
        .await;
}

/// 空闲等待结果：收到下一条输入（恢复运行）、通道关闭（shutdown）或待执行命令。
enum IdleResult {
    Resumed,
    Shutdown,
    /// idle gate 收到待执行命令（Compact / SwitchModel / …，#497 泛化载体）。
    CommandRequested(PendingCommand),
}

/// 检查当前 messages 是否有「待 assistant 响应的用户回合」：
/// 最后一条消息是 User 角色 → 有待答回合（true）；
/// 否则（空、末尾是 assistant / tool / system）→ 无待答回合（false）。
///
/// 用于 loop 顶部空闲门：若无待答回合且 pending_input 也为空，
/// 则先 idle-wait 直到收到真实 UserMessage 才开始回合。
fn has_pending_user_turn(messages: &[Message]) -> bool {
    matches!(messages.last(), Some(m) if m.role == Role::User)
}

/// 判断「本回合是否由一条真正的新用户消息开启」（#390 A1 跨回合泄漏修复用）。
///
/// 返回 `true` 仅当 `last` 是一条**真正的新用户输入**：
/// - role = User，且
/// - 不是工具结果消息（工具结果 role 虽为 User，但 `has_tool_results()` 为真——
///   这对应回合内的工具轮次再迭代，NEVER 视为新回合），且
/// - 不是 system-generated 用户消息（stop-hook 阻断注入的反馈，回合仍在继续）。
///
/// 用于在新 USER 回合边界（且仅在该边界）重置 `stall_detector` / `turn_start`，
/// 既消除跨回合泄漏，又保留单个回合内的 stall 检测能力。
pub(super) fn is_new_user_turn_message(last: Option<&Message>) -> bool {
    matches!(
        last,
        Some(m)
            if m.role == Role::User
                && !m.has_tool_results()
                && m.source() != share::message::MessageSource::SystemGenerated
    )
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

/// 读取共享槽里「当前回合 token」的 clone。
///
/// 锁仅在 clone 期间持有后立即释放（`std::sync::Mutex`，NEVER 跨 `.await`）。
/// `CancellationToken::clone` 共享内部取消状态：外部 `cancel_impl` 锁同一槽对
/// 当前 token 调 `cancel()` 后，本回合持有的 clone 同样变为已取消，从而被观测到。
fn current_cancel_token(slot: &std::sync::Mutex<CancellationToken>) -> CancellationToken {
    slot.lock()
        .map(|guard| guard.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
}

/// 将共享槽重置为一个全新的、未取消的 token。
///
/// 常驻 loop 处理完一次取消后调用：被取消的旧 token 已永久处于 cancelled 状态，
/// 若不替换，则下个回合从槽读到的仍是 cancelled token，会立即「胎死腹中」。
/// 替换为新 token 后，下回合 `current_cancel_token` 读到干净 token；同时 `cancel_impl`
/// 之后再触发取消会作用在这个新 token 上（针对的是「新一轮」工作，语义正确）。
fn reset_cancel(slot: &std::sync::Mutex<CancellationToken>) {
    let fresh = CancellationToken::new();
    match slot.lock() {
        Ok(mut guard) => *guard = fresh,
        Err(poisoned) => *poisoned.into_inner() = fresh,
    }
}

/// 进入空闲态：阻塞等待下一条「真正的新用户消息」，期间忽略不产生用户消息的事件。
///
/// 返回 `Resumed` 表示已有新用户消息 append 进 `messages`，调用方应恢复跑回合；
/// 返回 `Shutdown` 表示输入通道关闭，调用方应退出常驻 loop。
///
/// 空闲期语义：
/// - 单独 `ControlCommand`（/save、/model…）/ `Cancel` / `/clear` 都 append 0 条用户
///   消息 → 保持空闲，继续等下一条，NEVER 在陈旧历史上跑空回合。
///
/// `cancel_slot` 参数统一了两种调用场景（DRY，消除两个近乎相同的空闲函数）：
/// - `Some(slot)`：**回合完成 / cancel 后的空闲**。此时 loop 已经跑过 LLM/tool，
///   `cancel_impl` 可能已取消「当前槽里的 token」。为避免这枚 stale-cancelled token
///   污染随后真正恢复的回合，在「收到新用户消息恢复运行前」以及「空闲期观测到
///   abort/cancel 决策时」都 `reset_cancel`，保证新回合必从干净 token 起步。
/// - `None`：**loop 顶部首回合前置等待**。此时 loop 尚未开始任何 LLM 调用，
///   且 loop 体的 `cancel` clone 在本函数返回**之后**才从槽读取，故**不能** `reset_cancel`
///   ——重置会丢弃外部已经持有引用的 token，破坏首回合的外部 cancel 能力。
async fn idle_until_resume_or_shutdown<I, S>(
    input_events: &I,
    sink: &S,
    pending: &mut PendingInputBuffer,
    messages: &mut Vec<Message>,
    task_store: &storage::api::TaskStore,
    cancel_slot: Option<&std::sync::Mutex<CancellationToken>>,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    loop {
        match await_idle_input(input_events, pending).await {
            IdleResult::Resumed => {
                let gate = apply_gate(
                    GateKind::BeforeLlm,
                    pending,
                    sink,
                    messages,
                    &task_store,
                    true,
                )
                .await;
                if let Some(cmd) = gate.pending_command {
                    return IdleResult::CommandRequested(cmd);
                }
                if gate.appended_user_messages > 0 {
                    // 收到真正的新用户消息（已 append 进 messages）：恢复运行。
                    // 并发兜底：空闲期间外部可能对槽里的 token 直接调过 cancel()
                    // （`cancel_impl`，无对应输入事件经过本臂），使其变为已取消。
                    // 若不处理，下个真实回合会读到这枚 stale-cancelled token 而被误取消。
                    // 因此在恢复运行前无条件重置为干净 token（仅 `Some` 场景；首回合前置
                    // 等待 `None` 不重置——见函数文档）。
                    if let Some(slot) = cancel_slot {
                        reset_cancel(slot);
                    }
                    return IdleResult::Resumed;
                }
                if matches!(
                    gate.decision,
                    GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop
                ) {
                    // 空闲期取消（经输入通道的 Cancel/`/clear` 事件）：重置 token，
                    // 防止这枚已取消的 token 污染下一个真实回合（仅 `Some` 场景）。
                    if let Some(slot) = cancel_slot {
                        reset_cancel(slot);
                    }
                }
                // 0 append（命令 / 取消 / 空）→ 留在空闲，继续等下一条输入。
                continue;
            }
            IdleResult::Shutdown => return IdleResult::Shutdown,
            // `await_idle_input` 只返回 Resumed/Shutdown，CommandRequested 不可能到达。
            IdleResult::CommandRequested(cmd) => return IdleResult::CommandRequested(cmd),
        }
    }
}

/// 中止当前回合并回到空闲（常驻 actor 的取消语义）。
///
/// 取消不再退出 loop：本函数回滚本回合产生的消息、发出 `Cancelled`、**重置取消令牌**，
/// 然后经空闲机制阻塞等待下一条输入。返回 `Resumed`（收到新用户消息，调用方 `continue`
/// 跑新回合）或 `Shutdown`（输入通道关闭，调用方 `break` 退出 loop）。
///
/// 并发要点：必须在进入空闲*之前* `reset_cancel`，使下个回合从槽读到干净 token。
/// 重置后到进入空闲之间若发生 stale 的二次取消（外部对新 token 调 cancel），由空闲臂
/// （`idle_until_resume_or_shutdown` 中的 abort/cancel 决策分支）再次 reset 兜底。
#[allow(clippy::too_many_arguments)]
async fn cancel_to_idle<I, S>(
    sink: &S,
    input_events: &I,
    loop_fsm: &mut ChatLoopFsm,
    messages: &mut Vec<Message>,
    pending_input: &mut PendingInputBuffer,
    task_store: &storage::api::TaskStore,
    cancel_slot: &std::sync::Mutex<CancellationToken>,
    rollback_baseline: usize,
    turn_context: &RuntimeTurnContext,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    // 回滚到本回合基线（per-turn）：仅截掉当前回合产生的 assistant/tool 输出，
    // 保留本回合用户消息与所有先前已完成回合的消息，再同步给消费者。
    messages.truncate(rollback_baseline);
    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
        .await;
    sink.send_event(RuntimeStreamEvent::Cancelled {
        context: turn_context.clone(),
    })
    .await;
    // 重置取消令牌：被取消的旧 token 已永久 cancelled，必须换新 token 供下个回合。
    reset_cancel(cancel_slot);
    // FSM：回合中止 → 经 Stopping 进入 Idle（与回合完成后的空闲态共用 Idle 状态）。
    loop_fsm.transition(ChatLoopTransition::TryStop);
    loop_fsm.transition(ChatLoopTransition::Idle);
    loop_fsm.assert_state(ChatLoopState::Idle, "cancel aborts turn then idles");
    idle_until_resume_or_shutdown(
        input_events,
        sink,
        pending_input,
        messages,
        task_store,
        Some(cancel_slot),
    )
    .await
}

/// 应用 compact 结果到 loop 状态：冻结旧链 → 替换 messages → 设 summary → 发 MessagesSync。
async fn apply_compact_outcome<S>(
    sink: &S,
    outcome: CompactOutcome,
    messages: &mut Vec<Message>,
    frozen_chats: &Arc<std::sync::Mutex<Vec<crate::business::session::ChatSegment>>>,
    active_summary: &mut Option<String>,
    active_summary_arc: &Arc<std::sync::Mutex<Option<String>>>,
) where
    S: ChatEventSink,
{
    // 1. 冻结旧链
    let old_segment = {
        use crate::business::session::ChatSegment;
        let mut seg = ChatSegment::normal(None);
        seg.messages = std::mem::take(messages);
        seg
    };
    if let Ok(mut guard) = frozen_chats.lock() {
        guard.push(old_segment);
    }

    // 2. 替换为 recent tail
    *messages = outcome.messages;

    // 3. 设 summary
    *active_summary = Some(outcome.summary);
    if let Ok(mut guard) = active_summary_arc.lock() {
        *guard = active_summary.clone();
    }
    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
        .await;
}
