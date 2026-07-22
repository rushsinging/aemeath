use crate::application::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use crate::application::chat::looping::queue::{QueueDrainPort, QueueFuture};
use sdk::ChatInputEvent;
use share::message::Message;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;

pub type InputEventFuture<'a> = Pin<Box<dyn Future<Output = Vec<ChatInputEvent>> + Send + 'a>>;
pub type InputEventOptFuture<'a> =
    Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>;

/// [loop_debug] 返回 ChatInputEvent 的变体名（不含 payload），用于诊断日志。
/// 排查「无用户输入却持续跑」时，逐条打印 gate 收到的事件类型。
pub(crate) fn event_kind_name(event: &ChatInputEvent) -> &'static str {
    match event {
        ChatInputEvent::ControlCommand { .. } => "ControlCommand",
        ChatInputEvent::UserMessage { .. } => "UserMessage",
        ChatInputEvent::Reset => "Reset",
        ChatInputEvent::WithdrawAll => "WithdrawAll",
        ChatInputEvent::Compact => "Compact",
        ChatInputEvent::SwitchModel { .. } => "SwitchModel",
        ChatInputEvent::SetThinking { .. } => "SetThinking",
        ChatInputEvent::InitProject { .. } => "InitProject",
        ChatInputEvent::ManageSession { .. } => "ManageSession",
        ChatInputEvent::ManageMemory { .. } => "ManageMemory",
        ChatInputEvent::ResumeSession { .. } => "ResumeSession",
        ChatInputEvent::QueryReflectionHistory { .. } => "QueryReflectionHistory",
        ChatInputEvent::ListModels => "ListModels",
        ChatInputEvent::ListReminders => "ListReminders",
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
    BeforeFinish,
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
    /// 查询提醒列表。
    ListReminders,
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
    pub decision: GateDecision,
    pub commands: Vec<ControlCommand>,
    pub appended_user_messages: usize,
    pub dropped_events: usize,
    /// 本次 gate 采用的用户消息；调用方把它们绑定到下一 RunStep。
    pub adopted_messages: Vec<(sdk::InputId, Message)>,
    /// 本次 gate 采用的原始 UserMessage 事件（保留 InputId / text / images 三元组）。
    /// 供 RunPort 在 Run 首 step 的 accept_step_input 成功后 emit Adopted 时反查用。
    pub adopted_events: Vec<ChatInputEvent>,
    /// idle reset 已完成 Task 清理，请求 Context owner 清空 durable Session。
    pub reset_requested: bool,
    /// idle 时收到的待执行命令（替代 compact_requested + model_switch_requested）。
    pub pending_command: Option<PendingCommand>,
}

#[derive(Debug, Clone, Default)]
pub struct PendingInputBuffer {
    events: VecDeque<ChatInputEvent>,
}

impl PendingInputBuffer {
    pub fn push(&mut self, event: ChatInputEvent) {
        self.events.push_back(event);
    }

    pub fn extend(&mut self, events: impl IntoIterator<Item = ChatInputEvent>) {
        for event in events {
            self.push(event);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 批量取出并清空整个缓冲区（#391 S3：撤回 pending 输入用）。
    /// 空则返回空 Vec。
    pub fn drain_all(&mut self) -> Vec<ChatInputEvent> {
        self.events.drain(..).collect()
    }

    /// 取出所有 `UserMessage` 事件的文本，并从缓冲区移除这些事件。
    /// 非 UserMessage 事件（命令等）保留在缓冲区中。
    /// 用于 WithdrawAll：只撤回用户消息，保留命令待后续处理。
    pub fn drain_user_message_texts(&mut self) -> Vec<String> {
        let mut texts = Vec::new();
        let mut retained = VecDeque::new();
        while let Some(event) = self.events.pop_front() {
            match event {
                ChatInputEvent::UserMessage { text, .. } => texts.push(text),
                other => retained.push_back(other),
            }
        }
        self.events = retained;
        texts
    }

    /// 快照当前缓冲区中所有 `UserMessage` 事件（不修改缓冲区）。
    /// 用于 busy select! 期间 emit `UserMessagesQueued` 事件。
    pub fn user_message_snapshot(&self) -> Vec<(sdk::InputId, Message)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ChatInputEvent::UserMessage { id, text, .. } => {
                    Some((id.clone(), Message::user(text.clone())))
                }
                _ => None,
            })
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_loop_gate<Q, I, S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    queue: &Q,
    input_events: &I,
    sink: &S,
    task_access: &dyn task::TaskAccess,
    is_idle: bool,
) -> GateOutcome
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    drain_sources(buffer, queue, input_events).await;
    apply_gate(kind, buffer, sink, task_access, is_idle).await
}

pub async fn drain_sources<Q, I>(buffer: &mut PendingInputBuffer, queue: &Q, input_events: &I)
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    buffer.extend(input_events.drain_input_events().await);
    if let Some(queued) = queue.drain_queued_input().await {
        buffer.extend(
            queued
                .into_iter()
                .map(|text| ChatInputEvent::classify_text(text, Vec::new())),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_gate<S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
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
    let mut decision = GateDecision::Proceed;
    let mut pending_command: Option<PendingCommand> = None;
    let mut added: Vec<(sdk::InputId, Message)> = Vec::new();
    let mut added_events: Vec<ChatInputEvent> = Vec::new();
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
                    added.clear();
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
                added_events.push(ChatInputEvent::UserMessage {
                    id: id.clone(),
                    text: text.clone(),
                    images: images.clone(),
                });
                added.push(build_user_message(id, text, images));
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
                        dropped_events = iter.count();
                        decision = GateDecision::Proceed;
                        break;
                    }
                    // idle：权威 Task 清理成功后请求 Context owner 清空会话。
                    reset_requested = true;
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等回合结束回到 idle 再处理。
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
                // 回滚本批已 append 的 UserMessage（与 added 顺序一致）。
                if !added.is_empty() || !texts.is_empty() {
                    // added 逆序 = append 逆序，拼到 texts 前面保持原始提交顺序。
                    // 用 message.text_content() 还原用户视角文本（含 image placeholder）。
                    let mut all_texts: Vec<String> =
                        added.iter().map(|(_, m)| m.text_content()).collect();
                    all_texts.append(&mut texts);
                    // 本批消息尚未提交给 Context，清空 adopted 即完成回滚。
                    appended_user_messages = 0;
                    added.clear();
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
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等回合结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::Compact);
                }
            }
            ChatInputEvent::SwitchModel { selection } => {
                if is_idle {
                    pending_command = Some(PendingCommand::SwitchModel { selection });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等回合结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::SwitchModel { selection });
                }
            }
            ChatInputEvent::SetThinking { desired } => {
                if is_idle {
                    pending_command = Some(PendingCommand::SetThinking { desired });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等回合结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::SetThinking { desired });
                }
            }
            ChatInputEvent::InitProject { force } => {
                if is_idle {
                    pending_command = Some(PendingCommand::InitProject { force });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::InitProject { force });
                }
            }
            ChatInputEvent::ManageSession { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::ManageSession { args });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ManageSession { args });
                }
            }
            ChatInputEvent::ManageMemory { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::ManageMemory { args });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ManageMemory { args });
                }
            }
            ChatInputEvent::ResumeSession { id } => {
                if is_idle {
                    pending_command = Some(PendingCommand::ResumeSession { id });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ResumeSession { id });
                }
            }
            ChatInputEvent::QueryReflectionHistory { limit } => {
                if is_idle {
                    pending_command = Some(PendingCommand::QueryReflectionHistory { limit });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::QueryReflectionHistory { limit });
                }
            }
            ChatInputEvent::ListModels => {
                if is_idle {
                    pending_command = Some(PendingCommand::ListModels);
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ListModels);
                }
            }
            ChatInputEvent::ListReminders => {
                if is_idle {
                    pending_command = Some(PendingCommand::ListReminders);
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::ListReminders);
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
        decision = match kind {
            GateKind::AfterBlockingBoundary => GateDecision::Proceed,
            GateKind::BeforeLlm | GateKind::BeforeFinish => GateDecision::ContinueNextTurn,
        };
    }

    // [loop_debug] DEBUG 级：gate 最终决策 + 追加用户消息数。仅在有事件 / 有 append /
    // 非 Proceed 决策时打点，避免刷屏。默认不输出，调试时拉高级别可见。
    if event_count > 0 || appended_user_messages > 0 || decision != GateDecision::Proceed {
        log::debug!(
            target: crate::LOG_TARGET,
            "[loop_debug] apply_gate DONE kind={:?} decision={:?} appended_user_messages={} pending_command={:?}",
            kind, decision, appended_user_messages,
            pending_command.as_ref().map(|_| "some")
        );
    }

    GateOutcome {
        decision,
        commands,
        appended_user_messages,
        dropped_events,
        adopted_messages: added,
        adopted_events: added_events,
        reset_requested,
        pending_command,
    }
}

fn build_user_message(
    id: sdk::InputId,
    text: String,
    images: Vec<sdk::ChatInputImage>,
) -> (sdk::InputId, Message) {
    log::debug!(
        target: crate::LOG_TARGET,
        "[loop_debug] build_user_message id={} text_len={} image_count={}",
        id,
        text.len(),
        images.len()
    );
    let message = user_message_with_images(text, images);
    (id, message)
}

fn classify_control_command(raw: &str) -> ControlCommandKind {
    let command = raw.split_whitespace().next().unwrap_or_default();
    match command {
        "/clear" => ControlCommandKind::Abort,
        "/model" | "/provider" => ControlCommandKind::Reconfigure,
        _ => ControlCommandKind::SideEffect,
    }
}

pub(crate) fn user_message_with_images(text: String, images: Vec<sdk::ChatInputImage>) -> Message {
    if images.is_empty() {
        return Message::user(text);
    }
    // 事件携带的 (placeholder, base64, media_type) 三元组：
    // - placeholder 为 TUI 端 ImageSpan::placeholder() 生成的 `[Image #N]`
    // - Message::user_with_images 按 text 中 `[Image #N]` 出现顺序穿插拆块
    // - provider adapter 拿拆好的 Vec<ContentBlock>，无需再做拆分（#fix-tui-image-input-output）
    Message::user_with_images(
        text,
        images
            .into_iter()
            .map(|img| (img.id, img.base64, img.media_type))
            .collect(),
    )
}

#[derive(Clone, Default)]
pub struct EmptyInputEventDrainPort;

impl InputEventDrainPort for EmptyInputEventDrainPort {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
        Box::pin(async { Vec::new() })
    }

    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
        Box::pin(async { None })
    }
}

#[derive(Clone, Default)]
pub struct EmptyQueueDrainPort;

impl QueueDrainPort for EmptyQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
        Box::pin(async { None })
    }
}
