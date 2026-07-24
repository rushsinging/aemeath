use provider::ReasoningLevel;
use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
use share::message::Message;
use share::session_types::PersistedWorkspaceContext;
use std::future::Future;
use std::pin::Pin;
use tools::{AgentProgressEvent, ImageData};
use workflow::api::ReasoningNode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTurnContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl RuntimeTurnContext {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHookEventStatus {
    Running,
    Succeeded,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookExecutionResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub decision: Option<String>,
    pub reason: Option<String>,
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookEvent {
    pub hook_name: String,
    pub status: RuntimeHookEventStatus,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub result: Option<RuntimeHookExecutionResult>,
}

/// Hook 面向展示层的消息类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHookMessageKind {
    /// Hook JSON `additional_context`。
    AdditionalContext,
    /// Hook JSON `system_message`。
    SystemMessage,
}

/// Hook 面向展示层的结构化消息。
///
/// 该类型独立于 `SystemMessage`，使消费方能按 HookPoint、来源和 attempt 归因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookMessage {
    pub point: hook::HookPoint,
    pub source: String,
    pub execution_ordinal: u32,
    pub attempt: u8,
    pub kind: RuntimeHookMessageKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeToolCallStatus {
    PendingArgs,
    Ready,
    Running,
}

/// Compact 进度阶段（re-export from context crate）。
pub use context::compact::CompactStage;

#[derive(Debug, Clone)]
pub struct RuntimeResumedSessionStep {
    pub run_id: String,
    pub step_id: String,
    pub messages: Vec<Message>,
}

#[derive(Debug)]
pub enum RuntimeStreamEvent {
    Text {
        context: RuntimeTurnContext,
        text: String,
    },
    Thinking {
        context: RuntimeTurnContext,
        text: String,
    },
    BlockComplete {
        context: RuntimeTurnContext,
        text: String,
    },
    ToolCallStart {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: RuntimeToolCallStatus,
    },
    ToolResult {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<ImageData>,
    },
    SystemMessage(String),
    ModelStreamWaiting {
        context: RuntimeTurnContext,
        elapsed_secs: u64,
        phase: String,
    },
    ModelInvocationRetrying {
        context: RuntimeTurnContext,
        attempt: u32,
        delay: std::time::Duration,
    },
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    MicrocompactDone {
        messages: Vec<Message>,
        cleared_count: usize,
    },
    StopHookBlocked {
        messages: Vec<Message>,
    },
    PostToolExecutionSync {
        messages: Vec<Message>,
    },
    ApiError {
        messages: Vec<Message>,
        error: String,
    },
    CompactRollback {
        messages: Vec<Message>,
    },
    CompactFinished {
        messages: Vec<Message>,
    },
    TurnStarted {
        messages: Vec<Message>,
    },
    /// 用户输入被 gate 接纳（idle 直发或 batch drain）。
    /// items = 本批接纳的消息（InputId + 派生 Message）；
    /// queued = gate 处理后仍留在 buffer 中的排队消息快照（一般空，batch drain 时可能有剩余）。
    /// TUI 用 items 清占位/渲染正式消息，用 queued 重渲染 queue 区域。
    UserMessagesAdopted {
        items: Vec<(sdk::InputId, Message)>,
        queued: Vec<(sdk::InputId, Message)>,
    },
    /// busy 阶段收到新输入并存入 runtime 内部 buffer 后的确认。
    /// queued = 当前 buffer 全量快照。TUI 据此全量重渲染 queue 区域。
    UserMessagesQueued {
        queued: Vec<(sdk::InputId, Message)>,
    },
    /// loop 执行 reset 清理（messages + pending）后发出，通知 TUI 同步清镜像。
    SessionReset,
    /// 批量撤回 pending 输入：texts 为被撤回的 UserMessage 文本（#391 S3）。
    /// TUI 收到后清全部占位 + texts.join("\n") 还原输入框。
    UserMessagesWithdrawn {
        texts: Vec<String>,
    },
    Done {
        context: RuntimeTurnContext,
    },
    DoneWithDuration {
        context: RuntimeTurnContext,
        duration: std::time::Duration,
    },
    RunStarted {
        run_id: sdk::RunId,
        parent_run_id: Option<sdk::RunId>,
    },
    RunCancelling {
        run_id: sdk::RunId,
    },
    RunCancelled {
        run_id: sdk::RunId,
    },
    Cancelled {
        context: RuntimeTurnContext,
    },
    LiveTps(f64),
    TurnChanged(usize),
    HookEvent(RuntimeHookEvent),
    /// 结构化 hook 执行消息（typed projection）。
    HookMessage(RuntimeHookMessage),
    AskUserBatch {
        items: Vec<sdk::AskUserQuestionItem>,
        reply_tx: tokio::sync::oneshot::Sender<sdk::AskUserReply>,
    },
    /// #1246: typed interaction request (pure value, no sender).
    /// Production path replaces `AskUserBatch`.
    InteractionRequested {
        request: sdk::InteractionRequest,
    },
    AgentProgress {
        context: RuntimeTurnContext,
        tool_id: ToolCallId,
        event: AgentProgressEvent,
    },
    WorkingDirectoryChanged {
        path_base: String,
        workspace_root: String,
        workspace: PersistedWorkspaceContext,
    },
    /// 配置/指令/guidance 文件变更通知。
    ConfigReloaded {
        changed_keys: Vec<String>,
    },
    /// Reasoning Graph 阶段变化通知（Phase 2）。
    GraphPhaseChanged {
        node: ReasoningNode,
        effort: ReasoningLevel,
        prev: ReasoningNode,
    },
    /// Compact 进度通知。`current`/`total` 为 map-reduce chunk 计数（单次摘要时为 None）。
    CompactProgress {
        stage: CompactStage,
        current: Option<usize>,
        total: Option<usize>,
    },
    /// 模型切换完成通知（#567）。runtime idle 分支解析 selection 构建 client 后回传结果。
    ModelSwitched {
        result: sdk::ModelSwitchResult,
    },
    /// Reasoning 模式切换完成通知（#497）。runtime idle 分支执行 set_thinking 后回传结果。
    ThinkingChanged {
        enabled: bool,
    },
    /// 上下文估算完成通知（#497）。runtime idle 分支执行 estimate 后回传结果。
    ContextEstimated {
        estimate: sdk::ContextEstimate,
        message_count: usize,
    },
    /// 查询命令执行完成，返回纯文本结果（#497）。
    CommandResultText {
        text: String,
        is_error: bool,
    },
    /// 会话恢复完成通知（#497）。
    SessionResumed {
        steps: Vec<RuntimeResumedSessionStep>,
        session_id: String,
        created_at: u64,
    },
    /// 会话恢复失败（#636 D2）。区分 not_found / corrupt / io，前端展示对应错误。
    SessionResumeFailed {
        kind: sdk::SessionResumeFailureKind,
        id: String,
        message: String,
    },
    /// Reflection 历史安全视图回传（#899）。
    ReflectionHistory {
        records: Vec<sdk::ReflectionHistoryView>,
    },
    /// #567：模型列表回传。
    ModelList {
        models: Vec<sdk::ModelSummary>,
    },
    /// #567：提醒列表回传。
    ReminderList {
        reminders: Vec<sdk::ReminderView>,
    },
    /// #567：会话列表回传。
    SessionList {
        sessions: Vec<sdk::SessionSummary>,
    },
    /// #567：项目上下文回传。
    ProjectInfo {
        project: sdk::ProjectContext,
    },
    /// #567：任务状态快照回传（携带数据，替代轮询）。
    TasksSnapshot {
        tasks: Box<sdk::TaskStatusView>,
    },
    /// #567：成本信息回传。
    CostUpdate {
        cost: sdk::CostInfo,
    },
}

/// 判断 tool 名是否属于 task store mutation（会改变 task 状态）。
///
/// 用于 `TasksSnapshot` 事件推送触发点：只有 task mutation 工具执行后，
/// 才需要重新取 task snapshot 并推送给前端（#642）。
pub(crate) fn is_task_store_mutation(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "TaskCreate" | "TaskUpdate" | "TaskStop" | "TaskListCreate" | "TaskListComplete"
    )
}

pub trait ChatEventSink: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;

    fn try_send_event(&self, event: RuntimeStreamEvent);

    fn send_domain_event<'a>(
        &'a self,
        _event: crate::domain::agent_run::RunDomainEvent,
    ) -> EventFuture<'a> {
        Box::pin(async {})
    }
}

trait DynChatEventSink: Send + Sync {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;
    fn try_send_event(&self, event: RuntimeStreamEvent);
    fn send_domain_event<'a>(
        &'a self,
        event: crate::domain::agent_run::RunDomainEvent,
    ) -> EventFuture<'a>;
}

impl<S> DynChatEventSink for S
where
    S: ChatEventSink,
{
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        ChatEventSink::send_event(self, event)
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        ChatEventSink::try_send_event(self, event);
    }

    fn send_domain_event<'a>(
        &'a self,
        event: crate::domain::agent_run::RunDomainEvent,
    ) -> EventFuture<'a> {
        ChatEventSink::send_domain_event(self, event)
    }
}

#[derive(Clone)]
pub struct ChatEventSinkHandle {
    inner: std::sync::Arc<dyn DynChatEventSink>,
}

impl ChatEventSinkHandle {
    pub fn new<S>(sink: S) -> Self
    where
        S: ChatEventSink,
    {
        Self {
            inner: std::sync::Arc::new(sink),
        }
    }
}

impl ChatEventSink for ChatEventSinkHandle {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.inner.send_event(event)
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.inner.try_send_event(event);
    }

    fn send_domain_event<'a>(
        &'a self,
        event: crate::domain::agent_run::RunDomainEvent,
    ) -> EventFuture<'a> {
        self.inner.send_domain_event(event)
    }
}
