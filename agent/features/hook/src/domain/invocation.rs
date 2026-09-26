//! Hook 触发点与 typed 调用请求。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §2。
//! 使用 enum 绑定 HookPointData 与 payload，禁止 `point + 无约束 JSON` 形成非法组合。

use serde::{Deserialize, Serialize};

// ─── HookPointData ────────────────────────────────────────────────

/// Hook 触发点（26 个变体）。
///
/// 系统拥有，用户配置不可创建新 point。对外统一语言使用 `SubRun`；
/// adapter 可兼容 Claude Code 的 `SubagentStart/Stop` 名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookPointData {
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

// ─── HookInvocationData ───────────────────────────────────────────

/// Hook 调用请求（typed dispatch）。
///
/// 每个变体绑定 payload struct，消除 `point + 无约束 JSON` 的非法组合。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookInvocationData {
    // ── 前置闸门 ──
    PreToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
    },
    UserPromptSubmit {
        prompt: String,
    },
    PreCompact {
        run_steps: usize,
        messages_count: usize,
    },
    PermissionRequest {
        tool_name: String,
        permission_rule: String,
    },
    Elicitation {
        server_name: String,
        elicitation_text: String,
    },
    UserPromptExpansion {
        original_input: String,
        expanded_input: String,
    },
    // ── Stop 闸门 ──
    Stop {
        run_steps: usize,
    },
    // ── 后置增强 ──
    PostToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
        tool_output: String,
        is_error: bool,
    },
    PostToolUseFailure {
        tool_name: String,
        tool_input: serde_json::Value,
        error: String,
    },
    PostCompact {
        run_steps: usize,
        messages_before: usize,
        messages_after: usize,
    },
    PostToolBatch {
        tool_count: usize,
        summary: String,
    },
    ElicitationResult {
        server_name: String,
        user_response: String,
    },
    // ── 生命周期 ──
    SessionStart {
        session_id: String,
    },
    SessionEnd {
        session_id: String,
    },
    SubRunStart {
        prompt: String,
        system: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_spec: Option<String>,
    },
    SubRunStop {
        prompt: String,
        system: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_spec: Option<String>,
        result: String,
        run_steps: usize,
        is_error: bool,
    },
    TaskCreated {
        tool_input: serde_json::Value,
        tool_output: String,
    },
    TaskCompleted {
        tool_input: serde_json::Value,
        tool_output: String,
    },
    Notification {
        notification_text: String,
        notification_type: String,
    },
    InstructionsLoaded {
        file_path: String,
        instruction_type: String,
    },
    // ── 观察 ──
    StopFailure {
        run_steps: usize,
        error: String,
    },
    PermissionDenied {
        tool_name: String,
        permission_rule: String,
    },
    ConfigChange {
        config_file: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        changed_field: Option<String>,
    },
    CwdChanged {
        old_cwd: String,
        new_cwd: String,
    },
    FileChanged {
        file_path: String,
        change_type: String,
    },
    TeammateIdle {
        teammate_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        idle_reason: Option<String>,
    },
}

impl HookInvocationData {
    /// 返回该调用对应的触发点。
    pub fn point(&self) -> HookPointData {
        match self {
            Self::PreToolUse { .. } => HookPointData::PreToolUse,
            Self::UserPromptSubmit { .. } => HookPointData::UserPromptSubmit,
            Self::PreCompact { .. } => HookPointData::PreCompact,
            Self::PermissionRequest { .. } => HookPointData::PermissionRequest,
            Self::Elicitation { .. } => HookPointData::Elicitation,
            Self::UserPromptExpansion { .. } => HookPointData::UserPromptExpansion,
            Self::Stop { .. } => HookPointData::Stop,
            Self::PostToolUse { .. } => HookPointData::PostToolUse,
            Self::PostToolUseFailure { .. } => HookPointData::PostToolUseFailure,
            Self::PostCompact { .. } => HookPointData::PostCompact,
            Self::PostToolBatch { .. } => HookPointData::PostToolBatch,
            Self::ElicitationResult { .. } => HookPointData::ElicitationResult,
            Self::SessionStart { .. } => HookPointData::SessionStart,
            Self::SessionEnd { .. } => HookPointData::SessionEnd,
            Self::SubRunStart { .. } => HookPointData::SubRunStart,
            Self::SubRunStop { .. } => HookPointData::SubRunStop,
            Self::TaskCreated { .. } => HookPointData::TaskCreated,
            Self::TaskCompleted { .. } => HookPointData::TaskCompleted,
            Self::Notification { .. } => HookPointData::Notification,
            Self::InstructionsLoaded { .. } => HookPointData::InstructionsLoaded,
            Self::StopFailure { .. } => HookPointData::StopFailure,
            Self::PermissionDenied { .. } => HookPointData::PermissionDenied,
            Self::ConfigChange { .. } => HookPointData::ConfigChange,
            Self::CwdChanged { .. } => HookPointData::CwdChanged,
            Self::FileChanged { .. } => HookPointData::FileChanged,
            Self::TeammateIdle { .. } => HookPointData::TeammateIdle,
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
            Self::PreToolUse { tool_input, .. } => *tool_input = input.clone(),
            Self::UserPromptSubmit { prompt, .. } => *prompt = json_value_to_string(input),
            Self::PermissionRequest {
                permission_rule, ..
            } => *permission_rule = json_value_to_string(input),
            Self::Elicitation {
                elicitation_text, ..
            } => *elicitation_text = json_value_to_string(input),
            Self::UserPromptExpansion { expanded_input, .. } => {
                *expanded_input = json_value_to_string(input)
            }
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
