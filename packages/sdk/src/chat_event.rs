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
use crate::chat_result::{ChatResult, ToolResultImage};
use crate::chat_view::{
    AgentProgressEventView, SubRunActivityEventView, SubRunStartedEventView, WorkspaceContextView,
};
use crate::ChatMessage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

#[derive(Debug, Clone)]
pub struct LocalResumedSessionStep {
    pub run_id: String,
    pub step_id: String,
    pub message_segments: Vec<Arc<[share::message::Message]>>,
    pub finalize_cause: Option<ResumedStepFinalizeCause>,
    pub duration_ms: Option<u64>,
}

impl LocalResumedSessionStep {
    pub fn from_wire(step: ResumedSessionStep) -> Self {
        let messages = step
            .messages
            .into_iter()
            .map(sdk_message_to_local)
            .collect::<Vec<_>>();
        Self {
            run_id: step.run_id,
            step_id: step.step_id,
            message_segments: vec![messages.into()],
            finalize_cause: step.finalize_cause,
            duration_ms: step.duration_ms,
        }
    }

    pub fn messages(&self) -> impl Iterator<Item = &share::message::Message> {
        self.message_segments
            .iter()
            .flat_map(|segment| segment.iter())
    }

    pub fn materialize(&self) -> ResumedSessionStep {
        ResumedSessionStep {
            run_id: self.run_id.clone(),
            step_id: self.step_id.clone(),
            messages: self.messages().map(local_message_to_sdk).collect(),
            finalize_cause: self.finalize_cause,
            duration_ms: self.duration_ms,
        }
    }
}

fn sdk_message_to_local(message: ChatMessage) -> share::message::Message {
    share::message::Message {
        role: if message.role == "assistant" {
            share::message::Role::Assistant
        } else {
            share::message::Role::User
        },
        content: serde_json::from_value(serde_json::to_value(message.content).unwrap_or_default())
            .unwrap_or_default(),
        metadata: message
            .metadata
            .map(|metadata| share::message::MessageMetadata {
                source: match metadata.source {
                    crate::ChatMessageSource::User => share::message::MessageSource::User,
                    crate::ChatMessageSource::SystemGenerated => {
                        share::message::MessageSource::SystemGenerated
                    }
                    crate::ChatMessageSource::Hook => share::message::MessageSource::Hook,
                    crate::ChatMessageSource::SkillRequest => {
                        share::message::MessageSource::SkillRequest
                    }
                },
                hook_notice: metadata
                    .hook_notice
                    .map(|notice| share::message::HookNotice {
                        point: notice.point,
                        kind: match notice.kind {
                            crate::HookNoticeKindView::Blocked => {
                                share::message::HookNoticeKind::Blocked
                            }
                            crate::HookNoticeKindView::Failed => {
                                share::message::HookNoticeKind::Failed
                            }
                            crate::HookNoticeKindView::Info => share::message::HookNoticeKind::Info,
                        },
                        summary: notice.summary,
                        command: notice.command,
                        exit_code: notice.exit_code,
                        reason: notice.reason,
                        stdout_preview: notice.stdout_preview,
                        stderr_preview: notice.stderr_preview,
                        stdout_truncated: notice.stdout_truncated,
                        stderr_truncated: notice.stderr_truncated,
                        output_file: notice.output_file,
                    }),
                skill_request: metadata.skill_request.map(|payload| {
                    share::message::SkillRequestMetadata {
                        skill: payload.skill,
                        arguments: payload.arguments,
                        raw_input: payload.raw_input,
                    }
                }),
            }),
    }
}

fn local_message_to_sdk(message: &share::message::Message) -> ChatMessage {
    ChatMessage {
        role: match message.role {
            share::message::Role::User => "user".to_string(),
            share::message::Role::Assistant => "assistant".to_string(),
        },
        content: serde_json::from_value(serde_json::to_value(&message.content).unwrap_or_default())
            .unwrap_or_default(),
        metadata: message
            .metadata
            .as_ref()
            .map(|metadata| crate::ChatMessageMetadata {
                source: match metadata.source {
                    share::message::MessageSource::User => crate::ChatMessageSource::User,
                    share::message::MessageSource::SystemGenerated => {
                        crate::ChatMessageSource::SystemGenerated
                    }
                    share::message::MessageSource::Hook => crate::ChatMessageSource::Hook,
                    share::message::MessageSource::SkillRequest => {
                        crate::ChatMessageSource::SkillRequest
                    }
                },
                hook_notice: metadata
                    .hook_notice
                    .as_ref()
                    .map(|notice| crate::HookNoticeView {
                        point: notice.point.clone(),
                        kind: match notice.kind {
                            share::message::HookNoticeKind::Blocked => {
                                crate::HookNoticeKindView::Blocked
                            }
                            share::message::HookNoticeKind::Failed => {
                                crate::HookNoticeKindView::Failed
                            }
                            share::message::HookNoticeKind::Info => crate::HookNoticeKindView::Info,
                        },
                        summary: notice.summary.clone(),
                        command: notice.command.clone(),
                        exit_code: notice.exit_code,
                        reason: notice.reason.clone(),
                        stdout_preview: notice.stdout_preview.clone(),
                        stderr_preview: notice.stderr_preview.clone(),
                        stdout_truncated: notice.stdout_truncated,
                        stderr_truncated: notice.stderr_truncated,
                        output_file: notice.output_file.clone(),
                    }),
                skill_request: metadata.skill_request.as_ref().map(|payload| {
                    crate::SkillRequestMetadataView {
                        skill: payload.skill.clone(),
                        arguments: payload.arguments.clone(),
                        raw_input: payload.raw_input.clone(),
                    }
                }),
            }),
        input_id: None,
    }
}

#[derive(Debug, Clone)]
pub struct LocalSessionResumeBacking {
    pub steps: Vec<LocalResumedSessionStep>,
    pub display_history: Option<DisplayHistoryIndex>,
    pub session_id: String,
    pub created_at: u64,
    pub compacted: bool,
}

impl LocalSessionResumeBacking {
    pub fn from_wire(view: SessionResumeView) -> Self {
        Self {
            steps: view
                .steps
                .into_iter()
                .map(LocalResumedSessionStep::from_wire)
                .collect(),
            display_history: None,
            session_id: view.session_id,
            created_at: view.created_at,
            compacted: view.compacted,
        }
    }

    pub fn materialize(&self) -> SessionResumeView {
        SessionResumeView {
            steps: self
                .steps
                .iter()
                .map(LocalResumedSessionStep::materialize)
                .collect(),
            session_id: self.session_id.clone(),
            created_at: self.created_at,
            compacted: self.compacted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayHistoryWindowRequest {
    pub session_id: String,
    pub generation_revision: u64,
    pub member_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DisplayHistoryWindow {
    pub session_id: String,
    pub generation_revision: u64,
    pub steps: Vec<ResumedSessionStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayHistoryStepReference {
    pub run_id: String,
    pub step_id: String,
    pub member_name: String,
    pub estimated_lines: usize,
    #[serde(default)]
    pub user_input_history: Vec<String>,
    #[serde(default)]
    pub finalize_cause: Option<ResumedStepFinalizeCause>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayHistoryIndex {
    pub session_id: String,
    pub generation_revision: u64,
    pub steps: Vec<DisplayHistoryStepReference>,
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
    #[serde(default)]
    pub compacted: bool,
}

/// Runtime stream context used to bind UI events to the authoritative chat/run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChatEventContext {
    pub chat_id: crate::ids::ChatId,
    pub run_id: crate::ids::ChatRunId,
}

impl ChatEventContext {
    pub fn new(chat_id: crate::ids::ChatId, run_id: crate::ids::ChatRunId) -> Self {
        Self { chat_id, run_id }
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
    /// Assistant 文本增量。生产 mapper 只发布该显式 Subject + Delivery 事件。
    AssistantTextDelta {
        context: ChatEventContext,
        delta: String,
    },
    /// Thinking 文本增量。生产 mapper 只发布该显式 Delivery 事件。
    ThinkingDelta {
        context: ChatEventContext,
        delta: String,
    },
    /// Public wire compatibility：旧消费者仍可反序列化；生产 mapper 不再发布。
    Token {
        context: ChatEventContext,
        text: String,
    },
    /// Public wire compatibility：旧消费者仍可反序列化；生产 mapper 不再发布。
    Thinking {
        context: ChatEventContext,
        text: String,
    },
    /// 块完成。
    BlockComplete {
        context: ChatEventContext,
        text: String,
    },
    /// 工具调用已开始。生产 mapper 只发布该显式 fact。
    ToolCallStarted {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    /// Public wire compatibility：旧消费者仍可反序列化；生产 mapper 不再发布。
    ToolCallStart {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    /// 工具参数流增量。与状态事实分离，按 Provider stream order 拼接。
    ToolCallArgumentsDelta {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        delta: String,
    },
    /// 工具调用完整状态事实。arguments 是当前已验证的完整参数快照。
    ToolCallStateChanged {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments: Option<serde_json::Value>,
        status: ToolCallStatusView,
    },
    /// Public wire compatibility：旧消费者仍可反序列化；生产 mapper 不再发布。
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
    /// Run 启动，首次同步全量消息。TUI 据此启动 spinner(Thinking)。
    TurnStarted {
        messages: Vec<ChatMessage>,
    },
    /// Microcompact 已完成并清理陈旧 tool result；run 仍在进行。
    MicrocompactCompleted {
        messages: Vec<ChatMessage>,
        cleared_count: usize,
    },
    /// Public wire compatibility：旧消费者仍可读取；生产 mapper 不再发布。
    MicrocompactDone {
        messages: Vec<ChatMessage>,
        cleared_count: usize,
    },
    /// Runtime 已提交消息状态的轻量有序投影。
    SessionMessageStateChanged {
        message_count: usize,
        revision: u64,
    },
    /// Hook 用户可见 typed notice。
    HookNotice {
        notice: crate::HookNoticeView,
    },
    /// Provider API 调用失败。TUI 据此 stop spinner + 显示错误。
    ApiError {
        messages: Vec<ChatMessage>,
        error: String,
    },
    /// Compact operation 失败并回滚消息。
    CompactOperationRolledBack {
        messages: Vec<ChatMessage>,
    },
    /// Compact operation 成功完成；notice 是 Runtime-owned 的用户可见持久提示。
    CompactOperationCompleted {
        messages: Vec<ChatMessage>,
        notice: String,
    },
    /// Public wire compatibility：旧消费者仍可读取；生产 mapper 不再发布。
    CompactRollback {
        messages: Vec<ChatMessage>,
    },
    /// Public wire compatibility：旧消费者仍可读取；生产 mapper 不再发布。
    CompactFinished {
        messages: Vec<ChatMessage>,
        notice: String,
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
        terminal: crate::RunStepCancellationTerminal,
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
    /// Chat 被取消；由 Runtime 的 typed Run termination 投影。
    Cancelled {
        context: ChatEventContext,
        /// 取消前该run已经运行的耗时。
        duration_ms: u64,
    },
    /// 实时 TPS。
    LiveTps(f64),
    /// 当前 run 变化。
    RunChanged(usize),
    /// 记录当前 run 变化的端口事件。
    CurrentRunChanged(usize),
    /// Runtime-owned pure-value interaction request. Production waiter cutover is tracked by #878.
    InteractionRequested {
        request: crate::InteractionRequest,
    },
    /// Agent progress 事件，分别保留派生 Run 来源身份与父 ToolCall 挂载身份。
    AgentProgress {
        source_context: ChatEventContext,
        attachment_context: ChatEventContext,
        tool_id: crate::ids::ToolCallId,
        event: AgentProgressEventView,
    },
    /// 工具 stdout 流式输出增量（如 Bash 长输出命令）。
    ToolOutputDelta {
        context: ChatEventContext,
        tool_id: crate::ids::ToolCallId,
        delta: String,
    },
    /// Public wire compatibility：旧消费者仍可读取；生产 mapper 不再发布。
    ToolProgress {
        context: ChatEventContext,
        tool_id: crate::ids::ToolCallId,
        event: crate::chat_view::ToolProgressEventView,
    },
    SubRunStarted {
        event: SubRunStartedEventView,
    },
    /// Structured activity emitted by one Sub Run spawned from a Main Agent ToolCall.
    SubRunActivity {
        event: SubRunActivityEventView,
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
        display_history: Option<DisplayHistoryIndex>,
        session_id: String,
        created_at: u64,
        compacted: bool,
    },
    /// 会话恢复失败（#636 D2）。`kind` 区分 not_found / corrupt / io，    /// TUI 据此显示对应错误并恢复到空 session。
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
    TaskStateChanged {
        state: Box<crate::TaskStateView>,
    },
    RuntimeStatusChanged {
        status: Box<crate::RuntimeStatusView>,
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
