//! idle / cancel / resume 生命周期辅助逻辑。
//!
//! 从 `loop_runner.rs` 拆出：`IdleResult`、空闲等待、取消恢复、
//! `/think` 执行等辅助函数。主循环通过这些函数实现常驻 actor 语义。

use crate::business::chat::looping::apply_gate;
use crate::business::chat::looping::events::{
    ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::business::chat::looping::input_gate::{
    event_kind_name, GateDecision, GateKind, InputEventDrainPort, PendingCommand,
    PendingInputBuffer,
};
use crate::business::session::ChatChain;
use crate::LOG_TARGET;
use share::message::Message;
use share::message::Role;
use tokio_util::sync::CancellationToken;

/// idle 分支执行 `/think`：读当前 reasoning level，按 desired 设置新 level，
/// 发 `ThinkingChanged` + `SystemMessage`。
pub(crate) async fn execute_set_thinking<S>(
    client: &provider::api::LlmClient,
    sink: &S,
    desired: Option<bool>,
) where
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

/// 空闲等待结果：收到下一条输入（恢复运行）、通道关闭（shutdown）或待执行命令。
pub(crate) enum IdleResult {
    /// 收到新用户消息，已 append 到 chain。携带本 turn 的 segment ID。
    Resumed(String),
    Shutdown,
    /// idle gate 收到待执行命令（Compact / SwitchModel / …，#497 泛化载体）。
    CommandRequested(PendingCommand),
}

/// 检查当前 messages 是否有「待 assistant 响应的用户回合」：
/// 最后一条消息是 User 角色 → 有待答回合（true）；
/// 否则（空、末尾是 assistant / tool / system）→ 无待答回合（false）。
///
/// 用于 loop 顶部空闲门的后续迭代检查（#672 后仅首次迭代无条件 idle）：
/// completion arm Resumed 后 continue 回 loop-top 时，messages 已含 user tail，
/// 本函数返回 true → 不 double-idle。
pub(crate) fn has_pending_user_turn(messages: &[Message]) -> bool {
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
pub(crate) fn is_new_user_turn_message(last: Option<&Message>) -> bool {
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
            // [loop_debug] 空闲态被唤醒：记录到底是什么事件把 loop 从 idle 拉起来。
            // 若用户没输入却出现此日志，说明有事件被送进 input 通道（关键线索）。
            // DEBUG 级：默认不输出，排查 loop 自跑类问题时拉高级别可见。
            log::debug!(
                target: LOG_TARGET,
                "[loop_debug] await_idle_input WOKEN by event kind={}",
                event_kind_name(&event)
            );
            pending.push(event);
            IdleResult::Resumed(String::new())
        }
        None => {
            log::debug!(target: LOG_TARGET, "[loop_debug] await_idle_input channel closed → Shutdown");
            IdleResult::Shutdown
        }
    }
}

/// 读取共享槽里「当前回合 token」的 clone。
///
/// 锁仅在 clone 期间持有后立即释放（`std::sync::Mutex`，NEVER 跨 `.await`）。
/// `CancellationToken::clone` 共享内部取消状态：外部 `cancel_impl` 锁同一槽对
/// 当前 token 调 `cancel()` 后，本回合持有的 clone 同样变为已取消，从而被观测到。
pub(crate) fn current_cancel_token(
    slot: &std::sync::Mutex<CancellationToken>,
) -> CancellationToken {
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
pub(crate) fn reset_cancel(slot: &std::sync::Mutex<CancellationToken>) {
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
pub(crate) async fn idle_until_resume_or_shutdown<I, S>(
    input_events: &I,
    sink: &S,
    pending: &mut PendingInputBuffer,
    chain: &mut ChatChain,
    task_store: &storage::api::TaskStore,
    cancel_slot: Option<&std::sync::Mutex<CancellationToken>>,
) -> IdleResult
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    loop {
        match await_idle_input(input_events, pending).await {
            IdleResult::Resumed(_) => {
                // 生成新 segment ID（新 turn）
                let segment_id = sdk::ids::ChatId::new_v7().to_string();
                let gate = apply_gate(
                    GateKind::BeforeLlm,
                    pending,
                    sink,
                    chain,
                    &segment_id,
                    task_store,
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
                    return IdleResult::Resumed(segment_id);
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
pub(crate) async fn cancel_to_idle<I, S>(
    sink: &S,
    input_events: &I,
    loop_fsm: &mut crate::business::chat::looping::state::ChatLoopFsm,
    chain: &mut ChatChain,
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
    use crate::business::chat::looping::state::{ChatLoopState, ChatLoopTransition};

    // 回滚到本回合基线（per-turn）：仅截掉当前回合产生的 assistant/tool 输出，
    // 保留本回合用户消息与所有先前已完成回合的消息，再同步给消费者。
    chain.truncate_flat(rollback_baseline);
    let flat = chain.messages_flat();
    sink.send_event(RuntimeStreamEvent::CompactRollback { messages: flat })
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
        chain,
        task_store,
        Some(cancel_slot),
    )
    .await
}
