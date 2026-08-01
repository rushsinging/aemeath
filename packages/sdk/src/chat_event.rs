#[cfg(test)]
mod skills_updated_tests {
    #[test]
    fn snapshot_contains_metadata_and_routes_without_body_fields() {
        let event = crate::SkillsUpdatedEvent {
            revision: "r1".to_string(),
            skills: vec![crate::SkillView {
                name: "release".to_string(),
                aliases: Vec::new(),
                slash_command: Some("release".to_string()),
                slash_aliases: vec!["rel".to_string()],
                description: "release".to_string(),
                argument_hint: Some("[version]".to_string()),
            }],
            slash_routes: vec![crate::SkillSlashRouteView {
                skill: "release".to_string(),
                slash_command: "release".to_string(),
                aliases: vec!["rel".to_string()],
                argument_hint: Some("[version]".to_string()),
            }],
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("content"));
        assert!(!json.contains("source"));
        assert!(json.contains("revision"));
    }
}

use crate::activity::{ActivityChangeKind, ActivitySnapshotView, ActivityView};
use crate::chat::AskUserQuestionItem;
use crate::chat_result::{ChatResult, ToolResultImage};
use crate::chat_view::{
    AgentProgressEventView, HookEventView, HookMessageView, WorkspaceContextView,
};
use crate::ChatMessage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod run_status_view_tests {
    use super::{ChatEvent, RunStatusView};
    use serde_json::json;

    #[test]
    fn run_status_view_serializes_all_variants() {
        let statuses = [
            (RunStatusView::Created, "created"),
            (RunStatusView::DrainingInput, "draining_input"),
            (RunStatusView::PreparingContext, "preparing_context"),
            (RunStatusView::InvokingModel, "invoking_model"),
            (RunStatusView::ApplyingResponse, "applying_response"),
            (
                RunStatusView::AwaitingToolApproval,
                "awaiting_tool_approval",
            ),
            (RunStatusView::ExecutingTools, "executing_tools"),
            (RunStatusView::AwaitingUser, "awaiting_user"),
            (RunStatusView::Compacting, "compacting"),
            (RunStatusView::CancellingStep, "cancelling_step"),
            (RunStatusView::FinalizingStep, "finalizing_step"),
            (RunStatusView::Cancelling, "cancelling"),
            (RunStatusView::Terminating, "terminating"),
            (RunStatusView::Completed, "completed"),
            (RunStatusView::Failed, "failed"),
            (RunStatusView::Cancelled, "cancelled"),
            (RunStatusView::Terminated, "terminated"),
        ];

        for (status, expected) in statuses {
            assert_eq!(serde_json::to_value(status).unwrap(), json!(expected));
        }
    }

    #[test]
    fn run_transitioned_uses_typed_status() {
        let event = ChatEvent::RunTransitioned {
            run_id: crate::RunId::new_v7(),
            parent_run_id: None,
            status: RunStatusView::InvokingModel,
            timing: super::RunTimingView {
                observation_revision: 1,
                total_elapsed_ms: 12_345,
                phase_elapsed_ms: 678,
            },
        };

        match event {
            ChatEvent::RunTransitioned { status, .. } => {
                assert_eq!(status, RunStatusView::InvokingModel);
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ResumedStepFinalizeCause {
    Completed,
    UserCancelledStep,
    RunTerminated,
}

/// 会话恢复时由 Context 发布的完整用户可见 RunStep 历史投影。
/// compact 仅影响 Runtime active context，不过滤此展示投影。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResumedSessionStep {
    pub run_id: String,
    pub step_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub finalize_cause: Option<ResumedStepFinalizeCause>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// 启动 `--resume` 已完成一次 backing 恢复后交给前端的历史投影。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionResumeView {
    pub steps: Vec<ResumedSessionStep>,
    pub session_id: String,
    pub created_at: u64,
}

/// Runtime stream context used to bind UI events to the authoritative chat/turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChatEventContext {
    pub chat_id: crate::ids::ChatId,
    pub turn_id: crate::ids::ChatTurnId,
}

impl ChatEventContext {
    pub fn new(chat_id: crate::ids::ChatId, turn_id: crate::ids::ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunTimingView {
    pub observation_revision: u64,
    pub total_elapsed_ms: u64,
    pub phase_elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatusView {
    Created,
    DrainingInput,
    PreparingContext,
    InvokingModel,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    AwaitingUser,
    Compacting,
    CancellingStep,
    FinalizingStep,
    Cancelling,
    Terminating,
    Completed,
    Failed,
    Cancelled,
    Terminated,
}

/// 工具调用的中间状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatusView {
    PendingArgs,
    Ready,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionTriggerView {
    Interval,
    PreCompact,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionStatusView {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionApplyStatusView {
    NotApplied,
    Applied,
    PartiallyApplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionErrorCategoryView {
    LlmCall,
    EmptyResponse,
    Parse,
    InvalidSuggestion,
    Apply,
    History,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReflectionTokenUsageView {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Safe reflection history projection. It intentionally contains only metadata
/// and aggregate counts, never reflection output, prompts, or conversation text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReflectionHistoryView {
    pub id: String,
    pub timestamp: u64,
    pub trigger: ReflectionTriggerView,
    pub status: ReflectionStatusView,
    pub deviations: usize,
    pub suggestions: usize,
    pub outdated: usize,
    pub apply_status: ReflectionApplyStatusView,
    pub error_category: Option<ReflectionErrorCategoryView>,
    pub token_usage: Option<ReflectionTokenUsageView>,
    pub duration_ms: u64,
}

/// Chat 事件流中的单个事件。
#[derive(Debug)]
pub enum ChatEvent {
    /// Runtime Activity 的完整增量观测。
    ActivityChanged {
        kind: ActivityChangeKind,
        activity: ActivityView,
    },
    /// 单个 Run 在同一 revision 下的完整 Activity 快照。
    ActivitySnapshot(ActivitySnapshotView),
    SkillsUpdated {
        event: crate::tui::SkillsUpdatedEvent,
    },
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
    /// Runtime 将在延迟后发起新的模型调用 attempt。
    ModelInvocationRetrying {
        context: ChatEventContext,
        attempt: u32,
        delay: std::time::Duration,
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
        status: RunStatusView,
        timing: RunTimingView,
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
        /// 取消前该回合已经运行的耗时。
        duration_ms: u64,
    },
    /// 实时 TPS。
    LiveTps(f64),
    /// 当前 turn 变化。
    TurnChanged(usize),
    /// 记录当前 turn 变化的端口事件。
    CurrentTurnChanged(usize),
    /// Hook 事件。
    HookEvent(HookEventView),
    /// 结构化 hook 执行消息（typed projection）。
    HookMessage(HookMessageView),
    /// Runtime-owned pure-value interaction request. Production waiter cutover is tracked by #878.
    InteractionRequested {
        request: crate::InteractionRequest,
    },
    /// Legacy AskUser transport bridge. It remains reachable only until #878 switches production.
    AskUserBatch {
        items: Vec<AskUserQuestionItem>,
        /// 回传回答或显式取消。
        reply_tx: tokio::sync::oneshot::Sender<crate::AskUserReply>,
    },
    /// Agent progress 事件，分别保留派生 Run 来源身份与父 ToolCall 挂载身份。
    AgentProgress {
        source_context: ChatEventContext,
        attachment_context: ChatEventContext,
        tool_id: crate::ids::ToolCallId,
        event: AgentProgressEventView,
    },
    /// 工作目录变化。
    WorkingDirectoryChanged {
        path_base: String,
        workspace_root: String,
        workspace: WorkspaceContextView,
    },
    ConfigChanged {
        event: crate::ConfigChangedEvent,
    },
    ConfigReloaded {
        event: crate::ConfigReloadedEvent,
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
        steps: Vec<ResumedSessionStep>,
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
    /// Reflection 历史查询结果。记录仅含安全元数据和计数，不含正文。
    ReflectionHistory {
        records: Vec<ReflectionHistoryView>,
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionResumeFailureKind {
    /// session 文件不存在。
    NotFound,
    /// JSON 损坏且 .bak 回退失败。
    Corrupt,
    /// 底层 IO 错误。
    Io,
}
