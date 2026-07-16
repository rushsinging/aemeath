//! Chat 事件流类型：事件 / 上下文 / 工具调用状态。

use crate::chat::AskUserQuestionItem;
use crate::chat_result::{ChatResult, ToolResultImage};
use crate::chat_view::{AgentProgressEventView, HookEventView, WorkspaceContextView};
use crate::ChatMessage;
use serde::{Deserialize, Serialize};

/// Runtime stream context used to bind UI events to the authoritative chat/turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEventContext {
    pub chat_id: crate::ids::ChatId,
    pub turn_id: crate::ids::ChatTurnId,
}

impl ChatEventContext {
    pub fn new(chat_id: crate::ids::ChatId, turn_id: crate::ids::ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

/// 工具调用的中间状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatusView {
    PendingArgs,
    Ready,
    Running,
}

/// Chat 事件流中的单个事件。
#[derive(Debug)]
pub enum ChatEvent {
    /// LLM 返回的文本 token。
    Token {
        context: ChatEventContext,
        text: String,
    },
    /// LLM reasoning / thinking token。
    Thinking {
        context: ChatEventContext,
        text: String,
    },
    /// 块完成。
    BlockComplete {
        context: ChatEventContext,
        text: String,
    },
    /// 工具调用开始。
    ToolCallStart {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    /// 工具调用属性/状态更新。
    ToolCallUpdate {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: ToolCallStatusView,
    },
    /// 工具执行结果。
    ToolResult {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<ToolResultImage>,
    },
    /// 系统消息。
    SystemMessage(String),
    /// Stream is alive but no user-visible model delta has arrived yet.
    ModelStreamWaiting {
        context: ChatEventContext,
        elapsed_secs: u64,
        phase: String,
    },
    /// 用量统计。
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    /// Turn 启动，首次同步全量消息。TUI 据此启动 spinner(Thinking)。
    TurnStarted {
        messages: Vec<ChatMessage>,
    },
    /// Microcompact 清理了陈旧 tool result，turn 仍在进行。TUI 只同步消息，不动 spinner。
    MicrocompactDone {
        messages: Vec<ChatMessage>,
        cleared_count: usize,
    },
    /// Stop hook 阻止了 turn 结束，追加 system-reminder 后继续。TUI 只同步消息。
    StopHookBlocked {
        messages: Vec<ChatMessage>,
    },
    /// Tool 执行完成后的消息同步（AwaitUser gate）。TUI 只同步消息。
    PostToolExecutionSync {
        messages: Vec<ChatMessage>,
    },
    /// Provider API 调用失败。TUI 据此 stop spinner + 显示错误。
    ApiError {
        messages: Vec<ChatMessage>,
        error: String,
    },
    /// Compact 失败后回滚消息。TUI 只同步消息。
    CompactRollback {
        messages: Vec<ChatMessage>,
    },
    /// Compact（LLM 摘要）成功完成，替换消息列表。TUI 同步消息 + 清 compact 状态。
    CompactFinished {
        messages: Vec<ChatMessage>,
    },
    /// 用户输入被 gate 接纳（idle 直发或 batch drain）。
    /// items = 本批接纳的消息；queued = gate 处理后仍留在 buffer 中的排队消息快照（一般空）。
    /// TUI 用 items 清占位、用 queued 重渲染 queue 区域。
    UserMessagesAdopted {
        items: Vec<ChatMessage>,
        queued: Vec<ChatMessage>,
    },
    /// busy 阶段收到新输入，存入 runtime 内部 buffer 后的确认。
    /// queued = 当前 buffer 全量快照。TUI 据此全量重渲染 queue 区域。
    UserMessagesQueued {
        queued: Vec<ChatMessage>,
    },
    /// Chat 完成。
    Done {
        context: ChatEventContext,
    },
    /// Chat 完成并附带耗时毫秒。
    DoneWithDurationMs {
        context: ChatEventContext,
        duration_ms: u64,
    },
    /// Runtime 已创建并激活一个 Run。
    RunStarted {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
    },
    /// Runtime 已开始一个 Run Step。
    RunStepStarted {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        step_id: crate::RunStepId,
    },
    /// Runtime 已完成一个 Run Step。
    RunStepCompleted {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        step_id: crate::RunStepId,
    },
    RunStepCancellationRequested {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        step_id: crate::RunStepId,
    },
    RunStepFinalizationStarted {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        step_id: crate::RunStepId,
    },
    RunStepCancelled {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        step_id: crate::RunStepId,
        confirmed: bool,
    },
    RunDrainingInput {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
    },
    RunTerminationRequested {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        reason: crate::RunTerminationReason,
        deadline: crate::ControlDeadline,
    },
    RunTerminated {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        reason: crate::RunTerminationReason,
    },
    RunCompleted {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        result: String,
    },
    RunFailed {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        error: String,
    },
    RunStuckDetected {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        reason: String,
    },
    RunTransitioned {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
        status: String,
    },
    RunAwaitingUser {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
    },
    RunResumed {
        run_id: crate::RunId,
        parent_run_id: Option<crate::RunId>,
    },
    /// 同步打断请求已接受，Run 已进入 Cancelling。
    RunCancelling {
        run_id: crate::RunId,
    },
    /// Run 取消收口完成 ACK。
    RunCancelled {
        run_id: crate::RunId,
    },
    /// Chat 被取消（兼容旧 TUI 投影）。
    Cancelled {
        context: ChatEventContext,
    },
    /// 实时 TPS。
    LiveTps(f64),
    /// 当前 turn 变化。
    TurnChanged(usize),
    /// 记录当前 turn 变化的端口事件。
    CurrentTurnChanged(usize),
    /// Hook 事件。
    HookEvent(HookEventView),
    /// Runtime-owned pure-value interaction request. Production waiter cutover is tracked by #878.
    InteractionRequested {
        request: crate::InteractionRequest,
    },
    /// Legacy AskUser transport bridge. It remains reachable only until #878 switches production.
    AskUserBatch {
        items: Vec<AskUserQuestionItem>,
        /// 回传每个问题的答案（顺序与 items 一致）。
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Agent progress 事件投影。
    AgentProgress {
        context: ChatEventContext,
        tool_id: crate::ids::ToolCallId,
        event: AgentProgressEventView,
    },
    /// 工作目录变化。
    WorkingDirectoryChanged {
        path_base: String,
        workspace_root: String,
        workspace: WorkspaceContextView,
    },
    ConfigReloaded {
        changed_keys: Vec<String>,
    },
    /// loop 完成 reset 清理后发出，TUI 据此同步清空镜像。
    /// Reasoning Graph 阶段变化（Phase 2）。
    GraphPhaseChanged {
        node: String,
        effort: String,
        prev: String,
    },
    SessionReset,
    /// 批量撤回 pending 输入（#391 S3）。texts 为被撤回文本，TUI join("\n") 还原输入框。
    UserMessagesWithdrawn {
        texts: Vec<String>,
    },
    /// 兼容旧 ChatInput 流结果。
    Result(ChatResult),
    /// Compact 进度通知。
    CompactProgress {
        stage: String,
        current: Option<u32>,
        total: Option<u32>,
    },
    /// 模型切换完成通知（#497）。TUI 据此更新 5 个本地状态 + 回显。
    ModelSwitched {
        result: crate::ModelSwitchResult,
    },
    /// Reasoning 模式切换完成通知（#497）。TUI 据此更新 thinking 状态 + 回显。
    ThinkingChanged {
        enabled: bool,
    },
    /// 上下文估算完成通知（#497）。TUI 据此显示 token 占用信息。
    ContextEstimated {
        estimate: crate::ContextEstimate,
        message_count: usize,
    },
    /// 查询命令执行完成，返回纯文本结果（#497）。
    /// TUI 据此 append_system_notice 或 append_error_notice。
    CommandResultText {
        text: String,
        is_error: bool,
    },
    /// 会话恢复完成通知（#497）。TUI 据此更新 messages 和状态。
    SessionResumed {
        messages: Vec<ChatMessage>,
        session_id: String,
        created_at: u64,
    },
    /// 会话恢复失败（#636 D2）。`kind` 区分 not_found / corrupt / io，
    /// TUI 据此显示对应错误并恢复到空 session。
    SessionResumeFailed {
        kind: SessionResumeFailureKind,
        id: String,
        message: String,
    },
    /// #567：Reflection 结果回传。
    ReflectionResult {
        output: Box<crate::ReflectionOutputView>,
    },
    /// #567：模型列表回传。
    ModelList {
        models: Vec<crate::ModelSummary>,
    },
    /// #567：提醒列表回传。
    ReminderList {
        reminders: Vec<crate::ReminderView>,
    },
    /// #567：会话列表回传。
    SessionList {
        sessions: Vec<crate::SessionSummary>,
    },
    /// #567：项目上下文回传。
    ProjectInfo {
        project: crate::ProjectContext,
    },
    /// #567：任务状态快照回传（携带数据，替代轮询）。
    TasksSnapshot {
        tasks: Box<crate::TaskStatusView>,
    },
    /// #567：成本信息回传。
    CostUpdate {
        cost: crate::CostInfo,
    },
}

/// `SessionResumeFailed` 的失败分类（#636 D2）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionResumeFailureKind {
    /// session 文件不存在。
    NotFound,
    /// JSON 损坏且 .bak 回退失败。
    Corrupt,
    /// 底层 IO 错误。
    Io,
}
