use crate::application::loop_engine::chat::events::{ChatEventSink, RuntimeStreamEvent};
use sdk::ChatInputEvent;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type InputEventFuture<'a> = Pin<Box<dyn Future<Output = Vec<ChatInputEvent>> + Send + 'a>>;
pub type InputEventOptFuture<'a> =
    Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>;

/// [loop_debug] 返回 ChatInputEvent 的变体名（不含 payload），用于诊断日志。
/// 排查「无用户输入却持续跑」时，逐条打印 gate 收到的事件类型。
pub(crate) fn event_kind_name(event: &ChatInputEvent) -> &'static str {
    match event {
        ChatInputEvent::ControlCommand { .. } => "ControlCommand",
        ChatInputEvent::UserMessage { .. } => "UserMessage",
        ChatInputEvent::SkillRequest(_) => "SkillRequest",
        ChatInputEvent::Reset => "Reset",
        ChatInputEvent::WithdrawAll => "WithdrawAll",
        ChatInputEvent::Compact => "Compact",
        ChatInputEvent::ReflectNow => "ReflectNow",
        ChatInputEvent::SwitchModel { .. } => "SwitchModel",
        ChatInputEvent::SetThinking { .. } => "SetThinking",
        ChatInputEvent::InitProject { .. } => "InitProject",
        ChatInputEvent::ManageSession { .. } => "ManageSession",
        ChatInputEvent::ManageMemory { .. } => "ManageMemory",
        ChatInputEvent::ResumeSession { .. } => "ResumeSession",
        ChatInputEvent::QueryReflectionHistory { .. } => "QueryReflectionHistory",
        ChatInputEvent::ListModels => "ListModels",
    }
}

pub trait InputEventDrainPort: Clone + Send + Sync + 'static {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a>;
    /// 阻塞等待下一条输入；None = 通道关闭（shutdown）。
    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    BeforeLlm,
    #[cfg(test)]
    BeforeFinish,
    #[cfg(test)]
    AfterBlockingBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Proceed,
    ContinueNextTurn,
    AbortCurrentLoop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCommandKind {
    Abort,
    SideEffect,
    Reconfigure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCommand {
    pub raw: String,
    pub kind: ControlCommandKind,
}

/// idle gate 收到的待执行命令（由 slash 命令触发，#497）。
///
/// 泛化载体：新增命令只需加一个变体 + apply_gate idle 分支 + loop_runner 执行分支，
/// 不再散弹式修改多处 match 臂。
#[derive(Debug, Clone)]
pub enum PendingCommand {
    Compact,
    /// 立即执行一次 Reflection（/reflect-now）。
    ReflectNow,
    SwitchModel {
        selection: String,
    },
    SetThinking {
        desired: Option<bool>,
    },
    /// 初始化项目（/init）。
    InitProject {
        force: bool,
    },
    /// 管理会话（/session）。
    ManageSession {
        args: String,
    },
    /// 管理记忆（/memory 非 remind）。
    ManageMemory {
        args: String,
    },
    /// 恢复会话（/resume <id>）。
    ResumeSession {
        id: String,
    },
    /// 查询 reflection 历史；不触发执行或 apply。
    QueryReflectionHistory {
        limit: usize,
    },
    /// 查询模型列表。
    ListModels,
}

// #567: 手动实现 PartialEq/Eq，不比较变体内数据。
impl PartialEq for PendingCommand {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
impl Eq for PendingCommand {}

#[derive(Debug, Clone)]
pub struct GateOutcome {
    #[cfg(test)]
    pub decision: GateDecision,
    #[cfg(test)]
    pub commands: Vec<ControlCommand>,
    pub appended_user_messages: usize,
    #[cfg(test)]
    pub dropped_events: usize,
    /// 重新排队等待下一轮 idle 的事件数（#1816）。gate 只消费到第一个控制
    /// 命令，其余事件原序回到 `PendingInputBuffer`，NEVER 静默丢弃。
    #[cfg(test)]
    pub requeued_events: usize,
    /// 本次 gate 接纳的 typed 用户输入。它是后续模型消息、持久化与
    /// UserMessagesAdopted 的唯一真相，禁止并行维护 event/message 双轨。
    pub accepted_inputs: Vec<crate::application::loop_engine::AcceptedUserInput>,
    /// idle reset 已完成 TaskData 清理，请求 Context owner 清空 durable Session。
    pub reset_requested: bool,
    /// idle 时收到的待执行命令（替代 compact_requested + model_switch_requested）。
    pub pending_command: Option<PendingCommand>,
}

/// 缓冲区里的一项待处理输入：事件本身 + 它的入队序号（#1816）。
///
/// 序号是排队回显的排序依据：用户消息用事件自带的 `InputId`，控制类命令
/// 在入队时分配一个新的 v7 id，因此跨「消息队列 / 命令队列」按 id 排序
/// 等价于按提交顺序排序。
#[derive(Debug, Clone)]
struct QueuedInput {
    id: sdk::InputId,
    event: ChatInputEvent,
}

impl QueuedInput {
    fn new(event: ChatInputEvent) -> Self {
        let id = match &event {
            ChatInputEvent::UserMessage { id, .. } => id.clone(),
            ChatInputEvent::SkillRequest(request) => request.input_id.clone(),
            _ => sdk::InputId::new_v7(),
        };
        Self { id, event }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PendingInputBuffer {
    events: Arc<Mutex<VecDeque<QueuedInput>>>,
}

impl PendingInputBuffer {
    /// 入队一项待处理输入，返回它的入队序号。
    pub fn push(&self, event: ChatInputEvent) -> sdk::InputId {
        let queued = QueuedInput::new(event);
        let id = queued.id.clone();
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push_back(queued);
        id
    }

    /// 队列中的控制类命令全量快照：入队序号 + 展示文本（#1816）。
    ///
    /// 快照是排队回显的唯一数据源，调用方按它整列渲染，不自建增量状态。
    /// 用户消息与技能请求不在其中——它们由 `UserMessagesQueued` 承载。
    pub fn command_snapshot(&self) -> Vec<(sdk::InputId, String)> {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter_map(|queued| {
                queued
                    .event
                    .queue_display_text()
                    .map(|text| (queued.id.clone(), text))
            })
            .collect()
    }

    /// 撤回全部排队控制命令，返回它们的展示文本（#1816）。
    ///
    /// `WithdrawAll` 的语义是「撤回所有待处理输入」：控制命令尚未执行，
    /// 撤回无副作用，因此与用户消息同批撤回，Up 键才等于「全部撤回」。
    pub fn drain_for_withdraw(&self) -> Vec<String> {
        self.drain_all()
            .iter()
            .filter_map(ChatInputEvent::queue_display_text)
            .collect()
    }

    /// 批量取走未遍历的剩余事件，原序放回缓冲区等待下一轮 idle（#1816）。
    ///
    /// 与 `drain_all` 成对使用：gate 一次 drain 后只消费到第一个控制命令，
    /// 其余事件必须回到缓冲区，否则会静默丢失。回队事件重新分配入队序号，
    /// 因此 id 单调顺序始终等于队列顺序。
    pub fn requeue(&self, events: impl IntoIterator<Item = ChatInputEvent>) {
        let mut buffer = self
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        buffer.extend(events.into_iter().map(QueuedInput::new));
    }

    #[cfg(test)]
    pub fn extend(&self, events: impl IntoIterator<Item = ChatInputEvent>) {
        self.requeue(events);
    }

    pub fn is_empty(&self) -> bool {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    }

    pub(crate) fn pop_front(&self) -> Option<ChatInputEvent> {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .map(|queued| queued.event)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    /// 批量取出并清空整个缓冲区（#391 S3：撤回 pending 输入用）。
    /// 空则返回空 Vec。
    pub fn drain_all(&self) -> Vec<ChatInputEvent> {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
            .map(|queued| queued.event)
            .collect()
    }
}

#[cfg(test)]
pub async fn run_loop_gate<I, S>(
    kind: GateKind,
    buffer: &PendingInputBuffer,
    input_events: &I,
    sink: &S,
    task_access: &dyn task::TaskAccess,
    is_idle: bool,
) -> GateOutcome
where
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    drain_source(buffer, input_events).await;
    apply_gate(kind, buffer, sink, task_access, is_idle).await
}

#[cfg(test)]
pub async fn drain_source<I>(buffer: &PendingInputBuffer, input_events: &I)
where
    I: InputEventDrainPort,
{
    buffer.extend(input_events.drain_input_events().await);
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_gate<S>(
    kind: GateKind,
    buffer: &PendingInputBuffer,
    sink: &S,
    task_access: &dyn task::TaskAccess,
    is_idle: bool,
) -> GateOutcome
where
    S: ChatEventSink,
{
    let mut commands = Vec::new();
    let mut appended_user_messages = 0usize;
    let mut dropped_events = 0usize;
    let mut requeued_events = 0usize;
    let mut decision = GateDecision::Proceed;
    let mut pending_command: Option<PendingCommand> = None;
    let mut accepted_inputs = Vec::new();
    let mut reset_requested = false;

    let events = buffer.drain_all();
    let event_count = events.len();
    // [loop_debug] DEBUG 级诊断：列出本次 gate 收到的所有事件类型。排查「无用户输入
    // 却持续跑」时是关键证据——若含 UserMessage/其它事件，说明有输入被送进来（TUI 误发
    // / LLM 输出被当输入 / 队列重放）。默认级别不输出，`AEMEATH_LOG_LEVEL=debug`
    // 拉高即可见。日志写入 agent-runtime.log / tui.log。
    if event_count > 0 {
        let kinds: Vec<&str> = events.iter().map(event_kind_name).collect();
        log::debug!(
            target: crate::LOG_TARGET,
            "[loop_debug] apply_gate kind={:?} is_idle={} drained_events={} kinds={:?}",
            kind, is_idle, event_count, kinds
        );
    } else {
        log::debug!(
            target: crate::LOG_TARGET,
            "apply_gate kind={:?} is_idle={} drained_events=0",
            kind, is_idle
        );
    }
    let mut iter = events.into_iter().peekable();
    while let Some(event) = iter.next() {
        match event {
            ChatInputEvent::ControlCommand { raw } => {
                let kind = classify_control_command(&raw);
                commands.push(ControlCommand {
                    raw: raw.clone(),
                    kind: kind.clone(),
                });
                if kind == ControlCommandKind::Abort {
                    dropped_events = iter.count();
                    appended_user_messages = 0;
                    accepted_inputs.clear();
                    decision = GateDecision::AbortCurrentLoop;
                    break;
                }
            }
            ChatInputEvent::UserMessage { id, text, images } => {
                let text_len = text.len();
                let image_count = images.len();
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[loop_debug] apply_gate UserMessage id={} text_len={} image_count={}",
                    id,
                    text_len,
                    image_count
                );
                accepted_inputs.push(
                    crate::application::loop_engine::AcceptedUserInput::UserMessage {
                        input_id: id,
                        text,
                        images,
                    },
                );
                appended_user_messages += 1;
            }
            ChatInputEvent::SkillRequest(request) => {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "skill_request boundary=runtime_input_gate input_id={} skill={} arguments_len={} raw_input_len={} raw_input_preview={:?}",
                    request.input_id,
                    request.skill,
                    request.arguments.len(),
                    request.raw_input.len(),
                    request.raw_input.chars().take(120).collect::<String>()
                );
                accepted_inputs.push(
                    crate::application::loop_engine::AcceptedUserInput::SkillRequest(request),
                );
                appended_user_messages += 1;
            }
            ChatInputEvent::Reset => {
                if is_idle {
                    // TaskAccess is authoritative since #889. Complete the only
                    // fallible reset mutation before clearing conversation state,
                    // so revision exhaustion cannot leave a partial reset.
                    if let Err(error) = task_access.clear() {
                        // Clear is atomic; failure (only revision exhaustion for
                        // the in-memory backing) leaves authoritative state
                        // untouched. Do not emit SessionReset or clear the
                        // compatibility store while Tasks still exist.
                        log::error!(target: crate::LOG_TARGET, "failed to clear authoritative tasks: {error}");
                        requeued_events = requeue_remaining(buffer, &mut iter);
                        decision = GateDecision::Proceed;
                        break;
                    }
                    // idle：权威 TaskData 清理成功后请求 Context owner 清空会话。
                    reset_requested = true;
                    // `reset_requested` 在调用方优先于 `Resumed`，本批已接纳输入
                    // 不会进入 Context；必须原序回队，否则会被静默丢弃（#1816）。
                    requeued_events += requeue_accepted_inputs(buffer, &accepted_inputs);
                    accepted_inputs.clear();
                    requeued_events += requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等run 结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::Reset);
                }
            }
            ChatInputEvent::WithdrawAll => {
                // 收集本批剩余 UserMessage 的 text（WithdrawAll 之后的）。
                let mut texts: Vec<String> = iter
                    .filter_map(|ev| match ev {
                        ChatInputEvent::UserMessage { text, .. } => Some(text),
                        _ => None,
                    })
                    .collect();
                // 回滚本批已接纳的 typed 用户输入。
                if !accepted_inputs.is_empty() || !texts.is_empty() {
                    let mut all_texts: Vec<String> = accepted_inputs
                        .iter()
                        .map(crate::application::loop_engine::AcceptedUserInput::withdraw_text)
                        .collect();
                    all_texts.append(&mut texts);
                    // 本批消息尚未提交给 Context，清空 adopted 即完成回滚。
                    appended_user_messages = 0;
                    accepted_inputs.clear();
                    sink.send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts: all_texts })
                        .await;
                }
                dropped_events = 0;
                decision = GateDecision::Proceed;
                break;
            }
            ChatInputEvent::Compact => {
                if is_idle {
                    pending_command = Some(PendingCommand::Compact);
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等run 结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::Compact);
                }
            }
            ChatInputEvent::ReflectNow => {
                if is_idle {
                    pending_command = Some(PendingCommand::ReflectNow);
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                }
                // busy：Manual 触发 NEVER 排队；提示后丢弃本事件，
                // 不放回 buffer（与 Compact 的 busy 排队语义相反）。
                sink.send_event(RuntimeStreamEvent::CommandResultText {
                    text: "Reflection 已在运行或等待运行结束，已跳过本次手动触发；稍后再试。"
                        .to_string(),
                    is_error: false,
                })
                .await;
            }
            ChatInputEvent::SwitchModel { selection } => {
                if is_idle {
                    pending_command = Some(PendingCommand::SwitchModel { selection });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等run 结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::SwitchModel { selection });
                }
            }
            ChatInputEvent::SetThinking { desired } => {
                if is_idle {
                    pending_command = Some(PendingCommand::SetThinking { desired });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等run 结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::SetThinking { desired });
                }
            }
            ChatInputEvent::InitProject { force } => {
                if is_idle {
                    pending_command = Some(PendingCommand::InitProject { force });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::InitProject { force });
                }
            }
            ChatInputEvent::ManageSession { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::ManageSession { args });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ManageSession { args });
                }
            }
            ChatInputEvent::ManageMemory { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::ManageMemory { args });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ManageMemory { args });
                }
            }
            ChatInputEvent::ResumeSession { id } => {
                if is_idle {
                    pending_command = Some(PendingCommand::ResumeSession { id });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ResumeSession { id });
                }
            }
            ChatInputEvent::QueryReflectionHistory { limit } => {
                if is_idle {
                    pending_command = Some(PendingCommand::QueryReflectionHistory { limit });
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::QueryReflectionHistory { limit });
                }
            }
            ChatInputEvent::ListModels => {
                if is_idle {
                    pending_command = Some(PendingCommand::ListModels);
                    requeued_events = requeue_remaining(buffer, &mut iter);
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ListModels);
                }
            }
        }
    }

    if appended_user_messages > 0 {
        log::debug!(
            target: crate::LOG_TARGET,
            "[loop_debug] apply_gate adopted_user_messages count={} kind={:?} (Adopted deferred to accept_step_input)",
            appended_user_messages,
            kind
        );
    }

    if decision == GateDecision::Proceed && appended_user_messages > 0 {
        #[cfg(test)]
        {
            decision = match kind {
                GateKind::AfterBlockingBoundary => GateDecision::Proceed,
                GateKind::BeforeLlm | GateKind::BeforeFinish => GateDecision::ContinueNextTurn,
            };
        }
        #[cfg(not(test))]
        {
            let _ = kind;
            decision = GateDecision::ContinueNextTurn;
        }
    }

    // [loop_debug] DEBUG 级：gate 最终决策 + 追加用户消息数。仅在有事件 / 有 append /
    // 非 Proceed 决策时打点，避免刷屏。默认不输出，调试时拉高级别可见。
    // gate 是 pending_input 的收口：入队、busy 放回、idle 消费与重新排队
    // 都体现在这一份全量快照里，UI 只需按快照整列重渲染（#1816）。
    sink.send_event(RuntimeStreamEvent::ControlCommandsQueued {
        queued: buffer.command_snapshot(),
    })
    .await;

    if event_count > 0 || appended_user_messages > 0 || decision != GateDecision::Proceed {
        log::debug!(
            target: crate::LOG_TARGET,
            "[loop_debug] apply_gate DONE kind={:?} decision={:?} appended_user_messages={} pending_command={:?}",
            kind, decision, appended_user_messages,
            pending_command.as_ref().map(|_| "some")
        );
    }

    #[cfg(not(test))]
    let _ = (&commands, dropped_events, requeued_events);

    GateOutcome {
        #[cfg(test)]
        decision,
        #[cfg(test)]
        commands,
        appended_user_messages,
        #[cfg(test)]
        dropped_events,
        #[cfg(test)]
        requeued_events,
        accepted_inputs,
        reset_requested,
        pending_command,
    }
}

/// 把已接纳但本轮不会进入 Context 的输入原序放回等待缓冲区（#1816）。
///
/// `reset_requested` 优先于 `Resumed`，因此 idle Reset 分支必须回放本批输入；
/// 回退事件而非丢弃，输入顺序对用户保持可见。
fn requeue_accepted_inputs(
    buffer: &PendingInputBuffer,
    accepted: &[crate::application::loop_engine::AcceptedUserInput],
) -> usize {
    let events: Vec<ChatInputEvent> = accepted
        .iter()
        .cloned()
        .map(crate::application::loop_engine::AcceptedUserInput::into_event)
        .collect();
    let count = events.len();
    buffer.requeue(events);
    count
}

/// 把 gate 未消费的事件原序放回等待缓冲区，返回重新排队的事件数（#1816）。
///
/// gate 一轮只消费到第一个控制命令（`pending_command` 单值语义），其余事件
/// 必须回到 `PendingInputBuffer` 等待下一轮 idle；Abort 分支是唯一例外——
/// 用户主动中止时整批丢弃，并连同已接纳输入一起回滚。
fn requeue_remaining(
    buffer: &PendingInputBuffer,
    iter: &mut impl Iterator<Item = ChatInputEvent>,
) -> usize {
    let remaining: Vec<ChatInputEvent> = iter.by_ref().collect();
    let count = remaining.len();
    buffer.requeue(remaining);
    count
}

fn classify_control_command(raw: &str) -> ControlCommandKind {
    let command = raw.split_whitespace().next().unwrap_or_default();
    match command {
        "/clear" => ControlCommandKind::Abort,
        "/model" | "/provider" => ControlCommandKind::Reconfigure,
        _ => ControlCommandKind::SideEffect,
    }
}
