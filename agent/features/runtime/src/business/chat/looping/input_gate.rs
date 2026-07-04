use crate::business::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use crate::business::chat::looping::queue::{QueueDrainPort, QueueFuture};
use crate::LOG_TARGET;
use sdk::ChatInputEvent;
use share::message::Message;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;

pub type InputEventFuture<'a> = Pin<Box<dyn Future<Output = Vec<ChatInputEvent>> + Send + 'a>>;
pub type InputEventOptFuture<'a> =
    Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>;

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
    CancelCurrentLoop,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingCommand {
    Compact,
    SwitchModel {
        selection: String,
    },
    SetThinking {
        desired: Option<bool>,
    },
    EstimateContext,
    /// 查询类命令（/cost /status /config /stats）。
    QueryCost {
        args: String,
    },
    QueryStatus,
    QueryConfig {
        args: String,
    },
    QueryStats {
        args: String,
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
    /// 保存当前会话（/save）。
    SaveSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateOutcome {
    pub decision: GateDecision,
    pub commands: Vec<ControlCommand>,
    pub appended_user_messages: usize,
    pub dropped_events: usize,
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
}

#[allow(clippy::too_many_arguments)]
pub async fn run_loop_gate<Q, I, S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    queue: &Q,
    input_events: &I,
    sink: &S,
    messages: &mut Vec<Message>,
    task_store: &storage::api::TaskStore,
    is_idle: bool,
) -> GateOutcome
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
    S: ChatEventSink,
{
    drain_sources(buffer, queue, input_events).await;
    apply_gate(kind, buffer, sink, messages, task_store, is_idle).await
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

pub async fn apply_gate<S>(
    kind: GateKind,
    buffer: &mut PendingInputBuffer,
    sink: &S,
    messages: &mut Vec<Message>,
    task_store: &storage::api::TaskStore,
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

    let events = buffer.drain_all();
    let event_count = events.len();
    log::debug!(
        target: LOG_TARGET,
        "apply_gate kind={:?} is_idle={} drained_events={}",
        kind,
        is_idle,
        event_count
    );
    let mut appended_this_gate = 0usize;
    let mut iter = events.into_iter().peekable();
    while let Some(event) = iter.next() {
        match event {
            ChatInputEvent::Cancel => {
                decision = GateDecision::CancelCurrentLoop;
                break;
            }
            ChatInputEvent::ControlCommand { raw } => {
                let kind = classify_control_command(&raw);
                commands.push(ControlCommand {
                    raw: raw.clone(),
                    kind: kind.clone(),
                });
                if kind == ControlCommandKind::Abort {
                    dropped_events = iter.count();
                    for _ in 0..appended_this_gate {
                        messages.pop();
                    }
                    appended_user_messages = 0;
                    added.clear();
                    decision = GateDecision::AbortCurrentLoop;
                    break;
                }
            }
            ChatInputEvent::UserMessage { id, text, images } => {
                let text_preview: String = text.chars().take(60).collect();
                log::debug!(
                    target: LOG_TARGET,
                    "apply_gate UserMessage id={} text_preview={:?} image_count={}",
                    id,
                    text_preview,
                    images.len()
                );
                added.push(append_user_message(messages, id, text, images));
                appended_this_gate += 1;
                appended_user_messages += 1;
            }
            ChatInputEvent::Reset => {
                if is_idle {
                    // idle：立即清空会话并通知 UI（保持 idle，不退出 loop）。
                    messages.clear();
                    added.clear();
                    appended_user_messages = 0;
                    // #567 S5：task_store 清理从 TUI RPC 下沉到 gate（不再调 clear_tasks()）
                    task_store.clear().await;
                    sink.send_event(RuntimeStreamEvent::SessionReset).await;
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
                if appended_this_gate > 0 || !texts.is_empty() {
                    // added 逆序 = append 逆序，拼到 texts 前面保持原始提交顺序。
                    // 用 message.text_content() 还原用户视角文本（含 image placeholder）。
                    let mut all_texts: Vec<String> =
                        added.iter().map(|(_, m)| m.text_content()).collect();
                    all_texts.append(&mut texts);
                    // 回滚已 append 的 messages。
                    for _ in 0..appended_this_gate {
                        messages.pop();
                    }
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
            ChatInputEvent::EstimateContext => {
                if is_idle {
                    pending_command = Some(PendingCommand::EstimateContext);
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    // busy：放回 buffer，等回合结束回到 idle 再处理。
                    buffer.push(ChatInputEvent::EstimateContext);
                }
            }
            ChatInputEvent::QueryCost { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::QueryCost { args });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::QueryCost { args });
                }
            }
            ChatInputEvent::QueryStatus => {
                if is_idle {
                    pending_command = Some(PendingCommand::QueryStatus);
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::QueryStatus);
                }
            }
            ChatInputEvent::QueryConfig { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::QueryConfig { args });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::QueryConfig { args });
                }
            }
            ChatInputEvent::QueryStats { args } => {
                if is_idle {
                    pending_command = Some(PendingCommand::QueryStats { args });
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::QueryStats { args });
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
            ChatInputEvent::SaveSession => {
                if is_idle {
                    pending_command = Some(PendingCommand::SaveSession);
                    dropped_events = iter.count();
                    decision = GateDecision::Proceed;
                    break;
                } else {
                    buffer.push(ChatInputEvent::SaveSession);
                }
            }
        }
    }

    if appended_user_messages > 0 {
        log::debug!(
            target: LOG_TARGET,
            "apply_gate sending PostToolExecutionSync + UserMessagesAdded count={} kind={:?}",
            appended_user_messages,
            kind
        );
        sink.send_event(RuntimeStreamEvent::PostToolExecutionSync {
            messages: messages.clone(),
        })
        .await;
        sink.send_event(RuntimeStreamEvent::UserMessagesAdded { items: added })
            .await;
    }

    if decision == GateDecision::Proceed && appended_user_messages > 0 {
        decision = match kind {
            GateKind::AfterBlockingBoundary => GateDecision::Proceed,
            GateKind::BeforeLlm | GateKind::BeforeFinish => GateDecision::ContinueNextTurn,
        };
    }

    GateOutcome {
        decision,
        commands,
        appended_user_messages,
        dropped_events,
        pending_command,
    }
}

fn append_user_message(
    messages: &mut Vec<Message>,
    id: sdk::InputId,
    text: String,
    images: Vec<sdk::ChatInputImage>,
) -> (sdk::InputId, Message) {
    log::info!(target: LOG_TARGET, "{}",
        serde_json::to_string(&serde_json::json!({
            "event_type": "user_input",
            "text": &text,
            "image_count": images.len(),
        })).unwrap_or_default()
    );
    let message = user_message_with_images(text, images);
    messages.push(message.clone());
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

fn user_message_with_images(text: String, images: Vec<sdk::ChatInputImage>) -> Message {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::chat::looping::events::EventFuture;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    /// #567 S5：gate 测试用 task_store（run_loop_gate 新增参数）
    fn test_task_store() -> storage::api::TaskStore {
        storage::api::TaskStore::new()
    }

    /// Mock port backed by tokio mpsc; supports both drain and blocking recv.
    #[derive(Clone)]
    struct MockInputPort {
        rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ChatInputEvent>>>,
    }

    impl MockInputPort {
        fn new() -> (mpsc::UnboundedSender<ChatInputEvent>, Self) {
            let (tx, rx) = mpsc::unbounded_channel();
            (
                tx,
                Self {
                    rx: Arc::new(tokio::sync::Mutex::new(rx)),
                },
            )
        }
    }

    impl InputEventDrainPort for MockInputPort {
        fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
            Box::pin(async move {
                let mut rx = self.rx.lock().await;
                let mut events = Vec::new();
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
                events
            })
        }

        fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
            Box::pin(async move {
                let mut rx = self.rx.lock().await;
                rx.recv().await
            })
        }
    }

    #[tokio::test]
    async fn test_recv_next_input_returns_event_then_none_on_close() {
        // MockInputPort: 用 tokio::sync::mpsc 支持 recv_next
        let (tx, port) = MockInputPort::new();
        tx.send(ChatInputEvent::UserMessage {
            id: sdk::InputId::new_v7(),
            text: "hi".into(),
            images: vec![],
        })
        .unwrap();
        let first = port.recv_next_input().await;
        assert!(matches!(first, Some(ChatInputEvent::UserMessage { .. })));
        drop(tx); // 关闭通道
        let after_close = port.recv_next_input().await;
        assert!(after_close.is_none(), "通道关闭后返回 None=shutdown");
    }

    #[derive(Clone)]
    struct TestInputEventPort {
        events: Arc<Mutex<Vec<ChatInputEvent>>>,
    }

    impl TestInputEventPort {
        fn new(events: Vec<ChatInputEvent>) -> Self {
            Self {
                events: Arc::new(Mutex::new(events)),
            }
        }
    }

    impl InputEventDrainPort for TestInputEventPort {
        fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
            Box::pin(async move { self.events.lock().unwrap().drain(..).collect() })
        }

        fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
            Box::pin(async move {
                let mut events = self.events.lock().unwrap();
                if events.is_empty() {
                    None
                } else {
                    Some(events.remove(0))
                }
            })
        }
    }

    #[derive(Clone)]
    struct TestQueuePort {
        queued: Arc<Mutex<Option<Vec<String>>>>,
    }

    impl TestQueuePort {
        fn new(queued: Option<Vec<String>>) -> Self {
            Self {
                queued: Arc::new(Mutex::new(queued)),
            }
        }
    }

    impl QueueDrainPort for TestQueuePort {
        fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
            Box::pin(async move { self.queued.lock().unwrap().take() })
        }
    }

    #[derive(Clone, Default)]
    struct TestSink {
        events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
    }

    impl ChatEventSink for TestSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                self.events.lock().unwrap().push(event);
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[tokio::test]
    async fn test_run_loop_gate_before_finish_continues_on_user_message() {
        let mut buffer = PendingInputBuffer::default();
        let queue = EmptyQueueDrainPort;
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message("继续", Vec::new())]);
        let sink = TestSink::default();
        let mut messages = vec![Message::user("first")];

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeFinish,
            &mut buffer,
            &queue,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
        assert_eq!(outcome.appended_user_messages, 1);
        assert_eq!(messages.last().unwrap().text_content(), "继续");
        // 现在 append 时发 MessagesSync + UserMessagesAdded 两个事件
        assert_eq!(sink.events.lock().unwrap().len(), 2);
    }

    /// #402 回归 + #fix-tui-image-input-output 拆块回归：
    /// 带图 UserMessage 事件必须按 text 中 `[Image #N]` 占位符穿插组装，
    /// 而非把所有 image 堆到 content 头部、text 堆到末尾。
    #[tokio::test]
    async fn test_user_message_with_images_assembles_image_block() {
        use share::message::{ContentBlock, ImageSource};
        let img = sdk::ChatInputImage {
            id: "[Image #1]".to_string(),
            base64: "Zm9vYmFy".to_string(),
            media_type: "image/png".to_string(),
        };
        // text 含 `[Image #1]` 占位符，期望 image 穿插到 text 中占位位置
        let text_with_marker = "看[Image #1]这张图".to_string();
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
            text_with_marker.clone(),
            vec![img],
        )]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.appended_user_messages, 1);
        let last = messages.last().expect("应追加一条消息");
        // text_content 拼回完整文本（拆块后还原）
        assert_eq!(last.text_content(), text_with_marker);
        // 期望 content 是 [Text("看"), Image, Text("这张图")] 三块
        assert_eq!(
            last.content.len(),
            3,
            "期望拆成 3 块，实际={:?}",
            last.content
        );
        assert!(matches!(&last.content[0], ContentBlock::Text { text } if text == "看"));
        let has_image = last.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Image {
                    source: ImageSource::Base64 { data, media_type },
                    placeholder: Some(ph),
                } if data == "Zm9vYmFy" && media_type == "image/png" && ph == "[Image #1]"
            )
        });
        assert!(
            has_image,
            "带图 UserMessage 应组装出 base64 image block（带 placeholder），实际 content={:?}",
            last.content
        );
        assert!(matches!(&last.content[2], ContentBlock::Text { text } if text == "这张图"));
    }

    /// #fix-tui-image-input-output：多图按 text 中 `[Image #N]` 出现顺序穿插。
    #[tokio::test]
    async fn test_user_message_with_multiple_images_interleaves_by_placeholder() {
        use share::message::ContentBlock;
        let imgs = vec![
            sdk::ChatInputImage {
                id: "[Image #1]".to_string(),
                base64: "a".to_string(),
                media_type: "image/png".to_string(),
            },
            sdk::ChatInputImage {
                id: "[Image #2]".to_string(),
                base64: "b".to_string(),
                media_type: "image/jpeg".to_string(),
            },
        ];
        // text 中 [Image #2] 在 [Image #1] 前面，期望穿插顺序: [Image #2], [Image #1]
        let text = "B: [Image #2], A: [Image #1]".to_string();
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(text.clone(), imgs)]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let _ = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        let last = messages.last().expect("应追加一条消息");
        assert_eq!(last.text_content(), text);
        let placeholders: Vec<String> = last
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Image {
                    placeholder: Some(p),
                    ..
                } => Some(p.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            placeholders,
            vec!["[Image #2]".to_string(), "[Image #1]".to_string()],
            "image 应按 text 中 `[Image #N]` 出现顺序穿插，实际 blocks={:?}",
            last.content
        );
    }

    #[tokio::test]
    async fn test_run_loop_gate_after_blocking_appends_without_continue_decision() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message(
            "tool 后输入",
            Vec::new(),
        )]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::AfterBlockingBoundary,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::Proceed);
        assert_eq!(outcome.appended_user_messages, 1);
        assert_eq!(messages[0].text_content(), "tool 后输入");
    }

    #[tokio::test]
    async fn test_run_loop_gate_preserves_side_effect_command_order() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::user_message("text1", Vec::new()),
            ChatInputEvent::ControlCommand {
                raw: "/save".to_string(),
            },
            ChatInputEvent::user_message("text2", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::ContinueNextTurn);
        assert_eq!(outcome.commands.len(), 1);
        assert_eq!(outcome.commands[0].raw, "/save");
        assert_eq!(messages[0].text_content(), "text1");
        assert_eq!(messages[1].text_content(), "text2");
    }

    #[tokio::test]
    async fn test_run_loop_gate_cancel_overrides_user_message() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::Cancel,
            ChatInputEvent::user_message("ignored", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeFinish,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::CancelCurrentLoop);
        assert!(messages.is_empty());
        assert!(sink.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_run_loop_gate_clear_drops_following_events_and_prior_appends() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::user_message("text1", Vec::new()),
            ChatInputEvent::ControlCommand {
                raw: "/clear".to_string(),
            },
            ChatInputEvent::user_message("text2", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeFinish,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::AbortCurrentLoop);
        assert_eq!(outcome.dropped_events, 1);
        assert_eq!(outcome.commands[0].kind, ControlCommandKind::Abort);
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_apply_gate_emits_user_messages_added_batch_no_dedup() {
        let mut buffer = PendingInputBuffer::default();
        // 含重复文本：验证不去重
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::user_message("same", Vec::new()),
            ChatInputEvent::user_message("same", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.appended_user_messages, 2, "不去重：两条都 append");
        assert_eq!(messages.len(), 2);
        let added = sink.events.lock().unwrap().iter().find_map(|e| match e {
            RuntimeStreamEvent::UserMessagesAdded { items } => Some(items.clone()),
            _ => None,
        });
        let items = added.expect("应发出一个 UserMessagesAdded 批事件");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1.text_content(), "same");
        assert_eq!(items[1].1.text_content(), "same");
        assert_ne!(items[0].0, items[1].0, "每条提交一个独立 id");
    }

    #[tokio::test]
    async fn test_run_loop_gate_no_dedup_push_and_pull_same_text() {
        // 设计 §8：不去重。queue 和 input 各来一条相同文本，两条都 append。
        let mut buffer = PendingInputBuffer::default();
        let queue = TestQueuePort::new(Some(vec!["same".to_string()]));
        let input = TestInputEventPort::new(vec![ChatInputEvent::user_message("same", Vec::new())]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &queue,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false,
        )
        .await;

        assert_eq!(outcome.appended_user_messages, 2, "不去重：两条都 append");
        assert_eq!(messages.len(), 2);
    }

    /// #391 S1-3：idle gate 收到 Reset → 清空 messages + 发 SessionReset，保持 idle。
    #[tokio::test]
    async fn test_idle_gate_reset_clears_messages_and_emits_session_reset() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::Reset]);
        let sink = TestSink::default();
        let mut messages = vec![Message::user("old1"), Message::user("resp1")];

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            true, // idle
        )
        .await;

        assert_eq!(outcome.decision, GateDecision::Proceed);
        assert!(
            messages.is_empty(),
            "idle Reset 应清空所有消息，实际 {:?}",
            messages
        );
        let has_reset = sink
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, RuntimeStreamEvent::SessionReset));
        assert!(has_reset, "应发出 SessionReset 事件");
    }

    /// #391 S1-3：busy gate 收到 Reset → 放回 buffer，messages 不变，等 idle 处理。
    #[tokio::test]
    async fn test_busy_gate_reset_defers_to_buffer() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::Reset]);
        let sink = TestSink::default();
        let mut messages = vec![Message::user("old1")];

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false, // busy
        )
        .await;

        assert_eq!(messages.len(), 1, "busy gate 不应清空 messages");
        assert_eq!(buffer.len(), 1, "Reset 应留在 buffer 等待 idle");
        assert_eq!(outcome.decision, GateDecision::Proceed);
        let has_reset = sink
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, RuntimeStreamEvent::SessionReset));
        assert!(!has_reset, "busy gate 不应发 SessionReset");
    }

    /// #391 S1-3：idle Reset 后跟 UserMessage → 先清空再 append（Reset break，UserMessage 丢弃）。
    #[tokio::test]
    async fn test_idle_gate_reset_drops_following_events_in_same_batch() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::Reset,
            ChatInputEvent::user_message("after-reset", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = vec![Message::user("old1")];

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            true, // idle
        )
        .await;

        assert_eq!(outcome.dropped_events, 1, "Reset 后的 UserMessage 应被丢弃");
        assert!(messages.is_empty(), "Reset 清空后不应 append 后续消息");
    }

    /// #391 S3-1：drain_all 非空 → 返回全部事件 + buffer 清空。
    #[test]
    fn test_drain_all_returns_all_events_and_clears() {
        let mut buffer = PendingInputBuffer::default();
        let a = ChatInputEvent::user_message("aaa", Vec::new());
        let b = ChatInputEvent::user_message("bbb", Vec::new());
        buffer.push(a.clone());
        buffer.push(b.clone());

        let drained = buffer.drain_all();

        assert_eq!(drained.len(), 2);
        assert!(matches!(&drained[0], ChatInputEvent::UserMessage { text, .. } if text == "aaa"));
        assert!(matches!(&drained[1], ChatInputEvent::UserMessage { text, .. } if text == "bbb"));
        assert!(buffer.is_empty(), "drain_all 后 buffer 应为空");
    }

    /// #391 S3-1：drain_all 空 → 返回空 Vec，buffer 仍空。
    #[test]
    fn test_drain_all_empty_returns_empty_vec() {
        let mut buffer = PendingInputBuffer::default();
        let drained = buffer.drain_all();
        assert!(drained.is_empty());
        assert!(buffer.is_empty());
    }

    /// #391 S3-3：WithdrawAll 非空 → 回滚已 append + 收集剩余 text + 发 Withdrawn。
    #[tokio::test]
    async fn test_withdraw_all_non_empty_emits_withdrawn_with_texts() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::user_message("aaa", Vec::new()),
            ChatInputEvent::user_message("bbb", Vec::new()),
            ChatInputEvent::WithdrawAll,
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            true, // idle
        )
        .await;

        let texts = sink.events.lock().unwrap().iter().find_map(|e| match e {
            RuntimeStreamEvent::UserMessagesWithdrawn { texts } => Some(texts.clone()),
            _ => None,
        });
        let texts = texts.expect("应发出 UserMessagesWithdrawn");
        assert_eq!(
            texts,
            vec!["aaa".to_string(), "bbb".to_string()],
            "应收集所有 UserMessage text（含已 append 的）"
        );
        assert!(messages.is_empty(), "WithdrawAll 应回滚已 append 的消息");
        assert!(buffer.is_empty(), "buffer 应为空");
        assert_eq!(outcome.appended_user_messages, 0);
    }

    /// #391 S3-3：WithdrawAll 空（无 UserMessage）→ no-op（无事件）。
    #[tokio::test]
    async fn test_withdraw_all_empty_buffer_is_noop() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![ChatInputEvent::WithdrawAll]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            true,
        )
        .await;

        let has_withdrawn = sink
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, RuntimeStreamEvent::UserMessagesWithdrawn { .. }));
        assert!(!has_withdrawn, "无 UserMessage 时不应发 Withdrawn");
        assert_eq!(outcome.appended_user_messages, 0);
    }

    /// #391 S3-3：busy gate 也立即处理 WithdrawAll（回滚 + 收集，不延迟）。
    #[tokio::test]
    async fn test_busy_gate_withdraw_all_executes_immediately() {
        let mut buffer = PendingInputBuffer::default();
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::user_message("queued", Vec::new()),
            ChatInputEvent::WithdrawAll,
        ]);
        let sink = TestSink::default();
        let mut messages = vec![Message::user("existing")];

        let task_store = test_task_store();
        let outcome = run_loop_gate(
            GateKind::BeforeLlm,
            &mut buffer,
            &EmptyQueueDrainPort,
            &input,
            &sink,
            &mut messages,
            &task_store,
            false, // busy
        )
        .await;

        let texts = sink.events.lock().unwrap().iter().find_map(|e| match e {
            RuntimeStreamEvent::UserMessagesWithdrawn { texts } => Some(texts.clone()),
            _ => None,
        });
        assert!(texts.is_some(), "busy gate 也应处理 WithdrawAll");
        assert_eq!(
            texts.unwrap(),
            vec!["queued".to_string()],
            "只撤回本批 UserMessage"
        );
        // existing 保留（上一回合的），queued 被回滚
        assert_eq!(messages.len(), 1, "queued 应被回滚，只保留 existing");
        assert_eq!(messages[0].text_content(), "existing");
        assert_eq!(outcome.appended_user_messages, 0);
    }
}
