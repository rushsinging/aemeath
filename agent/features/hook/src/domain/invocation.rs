//! Hook 触发点与 typed 调用请求。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §2。
//! 使用 enum 绑定 HookPoint 与 payload，禁止 `point + 无约束 JSON` 形成非法组合。

use serde::{Deserialize, Serialize};

// ─── HookPoint ────────────────────────────────────────────────

/// Hook 触发点（26 个变体）。
///
/// 系统拥有，用户配置不可创建新 point。对外统一语言使用 `SubRun`；
/// adapter 可兼容 Claude Code 的 `SubagentStart/Stop` 名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookPoint {
    // ── 前置闸门 ──
    PreToolUse,
    UserPromptSubmit,
    PreCompact,
    PermissionRequest,
    Elicitation,
    UserPromptExpansion,
    // ── Stop 闸门 ──
    Stop,
    // ── 后置增强 ──
    PostToolUse,
    PostToolUseFailure,
    PostCompact,
    PostToolBatch,
    ElicitationResult,
    // ── 生命周期 ──
    SessionStart,
    SessionEnd,
    SubRunStart,
    SubRunStop,
    TaskCreated,
    TaskCompleted,
    Notification,
    InstructionsLoaded,
    // ── 观察 ──
    StopFailure,
    PermissionDenied,
    ConfigChange,
    CwdChanged,
    FileChanged,
    TeammateIdle,
}

// ─── HookInvocation ───────────────────────────────────────────

/// Hook 调用请求（typed dispatch）。
///
/// 每个变体绑定 payload struct，消除 `point + 无约束 JSON` 的非法组合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookInvocation {
    // ── 前置闸门 ──
    PreToolUse(PreToolUseInput),
    UserPromptSubmit(UserPromptInput),
    PreCompact(PreCompactInput),
    PermissionRequest(PermissionInput),
    Elicitation(ElicitationInput),
    UserPromptExpansion(UserPromptExpansionInput),
    // ── Stop 闸门 ──
    Stop(StopInput),
    // ── 后置增强 ──
    PostToolUse(PostToolUseInput),
    PostToolUseFailure(PostToolUseFailureInput),
    PostCompact(PostCompactInput),
    PostToolBatch(PostToolBatchInput),
    ElicitationResult(ElicitationResultInput),
    // ── 生命周期 ──
    SessionStart(SessionInput),
    SessionEnd(SessionInput),
    SubRunStart(SubRunInput),
    SubRunStop(SubRunStopInput),
    TaskCreated(TaskInput),
    TaskCompleted(TaskInput),
    Notification(NotificationInput),
    InstructionsLoaded(InstructionsInput),
    // ── 观察 ──
    StopFailure(StopFailureInput),
    PermissionDenied(PermissionInput),
    ConfigChange(ConfigChangeInput),
    CwdChanged(CwdChangedInput),
    FileChanged(FileChangedInput),
    TeammateIdle(TeammateIdleInput),
}

impl HookInvocation {
    /// 返回该调用对应的触发点。
    pub fn point(&self) -> HookPoint {
        match self {
            Self::PreToolUse(_) => HookPoint::PreToolUse,
            Self::UserPromptSubmit(_) => HookPoint::UserPromptSubmit,
            Self::PreCompact(_) => HookPoint::PreCompact,
            Self::PermissionRequest(_) => HookPoint::PermissionRequest,
            Self::Elicitation(_) => HookPoint::Elicitation,
            Self::UserPromptExpansion(_) => HookPoint::UserPromptExpansion,
            Self::Stop(_) => HookPoint::Stop,
            Self::PostToolUse(_) => HookPoint::PostToolUse,
            Self::PostToolUseFailure(_) => HookPoint::PostToolUseFailure,
            Self::PostCompact(_) => HookPoint::PostCompact,
            Self::PostToolBatch(_) => HookPoint::PostToolBatch,
            Self::ElicitationResult(_) => HookPoint::ElicitationResult,
            Self::SessionStart(_) => HookPoint::SessionStart,
            Self::SessionEnd(_) => HookPoint::SessionEnd,
            Self::SubRunStart(_) => HookPoint::SubRunStart,
            Self::SubRunStop(_) => HookPoint::SubRunStop,
            Self::TaskCreated(_) => HookPoint::TaskCreated,
            Self::TaskCompleted(_) => HookPoint::TaskCompleted,
            Self::Notification(_) => HookPoint::Notification,
            Self::InstructionsLoaded(_) => HookPoint::InstructionsLoaded,
            Self::StopFailure(_) => HookPoint::StopFailure,
            Self::PermissionDenied(_) => HookPoint::PermissionDenied,
            Self::ConfigChange(_) => HookPoint::ConfigChange,
            Self::CwdChanged(_) => HookPoint::CwdChanged,
            Self::FileChanged(_) => HookPoint::FileChanged,
            Self::TeammateIdle(_) => HookPoint::TeammateIdle,
        }
    }

    /// 将一次 UpdatedInput 的值**整体替换**到本 invocation 对应的可修改 payload 字段，
    /// 再由调用方重新序列化传给下一条 subscription（设计 §10「UpdatedInput 串联」）。
    ///
    /// 仅对 `can_modify_input=true` 的 point 生效；其余变体（含不可修改 point）
    /// 由 `classify_directive` 提前拒绝，不会进入本方法。被替换的字段：
    /// - `PreToolUse.tool_input`（`serde_json::Value`，整体替换）；
    /// - `UserPromptSubmit.prompt` / `PermissionRequest.permission_rule` /
    ///   `Elicitation.elicitation_text` / `UserPromptExpansion.expanded_input`
    ///   （`String`：JSON 字符串直接取内串，其它形态取其 JSON 文本表示）。
    ///
    /// **NEVER** 仅往 enum JSON 顶层插键——payload 结构位置必须保持稳定。
    pub(crate) fn apply_updated_input(&mut self, input: &serde_json::Value) {
        match self {
            Self::PreToolUse(i) => i.tool_input = input.clone(),
            Self::UserPromptSubmit(i) => i.prompt = json_value_to_string(input),
            Self::PermissionRequest(i) => i.permission_rule = json_value_to_string(input),
            Self::Elicitation(i) => i.elicitation_text = json_value_to_string(input),
            Self::UserPromptExpansion(i) => i.expanded_input = json_value_to_string(input),
            // 不可修改 point：classify_directive 已拒绝，理论不可达。
            _ => {}
        }
    }
}

/// 将 JSON 值规约为字符串：字符串取内串，其余取 JSON 文本表示。
fn json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ─── Typed Input Structs ──────────────────────────────────────

// ── 前置闸门 ──

/// PreToolUse 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseInput {
    /// 工具名。
    pub tool_name: String,
    /// 工具输入参数（JSON）。
    pub tool_input: serde_json::Value,
}

/// UserPromptSubmit 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptInput {
    /// 用户输入文本。
    pub prompt: String,
}

/// PreCompact 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreCompactInput {
    /// agent 循环执行的轮次。
    pub turns: usize,
    /// 压缩前消息数量。
    pub messages_count: usize,
}

/// PermissionRequest / PermissionDenied 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionInput {
    /// 工具名。
    pub tool_name: String,
    /// 权限规则。
    pub permission_rule: String,
}

/// Elicitation 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationInput {
    /// MCP 服务器名。
    pub server_name: String,
    /// 请求文本。
    pub elicitation_text: String,
}

/// UserPromptExpansion 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptExpansionInput {
    /// 原始用户输入。
    pub original_input: String,
    /// 展开后的输入。
    pub expanded_input: String,
}

// ── Stop 闸门 ──

/// Stop 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopInput {
    /// agent 循环执行的轮次。
    pub turns: usize,
}

// ── 后置增强 ──

/// PostToolUse 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseInput {
    /// 工具名。
    pub tool_name: String,
    /// 工具输入参数（JSON）。
    pub tool_input: serde_json::Value,
    /// 工具执行结果。
    pub tool_output: String,
    /// 是否为错误结果。
    pub is_error: bool,
}

/// PostToolUseFailure 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseFailureInput {
    /// 工具名。
    pub tool_name: String,
    /// 工具输入参数（JSON）。
    pub tool_input: serde_json::Value,
    /// 失败错误信息。
    pub error: String,
}

/// PostCompact 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostCompactInput {
    /// agent 循环执行的轮次。
    pub turns: usize,
    /// 压缩前消息数量。
    pub messages_before: usize,
    /// 压缩后消息数量。
    pub messages_after: usize,
}

/// PostToolBatch 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolBatchInput {
    /// 批量工具数量。
    pub tool_count: usize,
    /// 批量执行摘要。
    pub summary: String,
}

/// ElicitationResult 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResultInput {
    /// MCP 服务器名。
    pub server_name: String,
    /// 用户响应。
    pub user_response: String,
}

// ── 生命周期 ──

/// SessionStart / SessionEnd 输入。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInput {}

/// SubRunStart 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubRunInput {
    /// sub-run 的输入提示。
    pub prompt: String,
    /// 系统消息。
    pub system: String,
    /// 使用的模型规格（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_spec: Option<String>,
}

/// SubRunStop 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubRunStopInput {
    /// sub-run 的输入提示。
    pub prompt: String,
    /// 系统消息。
    pub system: String,
    /// 使用的模型规格（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_spec: Option<String>,
    /// 执行结果。
    pub result: String,
    /// 执行的轮次。
    pub turns: usize,
    /// 是否为错误结果。
    pub is_error: bool,
}

/// TaskCreated / TaskCompleted 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    /// 工具输入参数（JSON）。
    pub tool_input: serde_json::Value,
    /// 工具执行结果。
    pub tool_output: String,
}

/// Notification 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationInput {
    /// 通知文本。
    pub notification_text: String,
    /// 通知类型。
    pub notification_type: String,
}

/// InstructionsLoaded 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionsInput {
    /// 文件路径。
    pub file_path: String,
    /// 指令类型（"claude_md" / "guidance"）。
    pub instruction_type: String,
}

// ── 观察 ──

/// StopFailure 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopFailureInput {
    /// agent 循环执行的轮次。
    pub turns: usize,
    /// 导致停止失败的错误信息。
    pub error: String,
}

/// ConfigChange 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeInput {
    /// 配置文件。
    pub config_file: String,
    /// 变更的字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_field: Option<String>,
}

/// CwdChanged 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdChangedInput {
    /// 旧工作目录。
    pub old_cwd: String,
    /// 新工作目录。
    pub new_cwd: String,
}

/// FileChanged 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedInput {
    /// 文件路径。
    pub file_path: String,
    /// 变更类型。
    pub change_type: String,
}

/// TeammateIdle 输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateIdleInput {
    /// 队友名称。
    pub teammate_name: String,
    /// 空闲原因。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_reason: Option<String>,
}
