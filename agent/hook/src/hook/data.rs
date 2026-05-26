//! Hook 事件数据模型

use serde::{Deserialize, Serialize};
use share::config::hooks::HookEvent;

/// hook 输入数据（通过 stdin 传递给 hook 命令）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// 触发的事件类型
    pub event: HookEvent,
    /// 事件特定数据
    #[serde(flatten)]
    pub data: HookData,
}

/// 事件特定数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookData {
    /// PreToolUse / PostToolUse 数据
    Tool(ToolHookData),
    /// UserPrompt 数据
    Prompt(PromptHookData),
    /// Stop 事件数据
    Stop(StopHookData),
    /// SessionStart 事件数据
    Session(SessionHookData),
    /// PreCompact / PostCompact 事件数据
    Compact(CompactHookData),
    /// SubagentStart / SubagentStop 事件数据
    Subagent(SubagentHookData),
    // ========== P2 事件数据 ==========
    /// PermissionRequest / PermissionDenied 事件数据
    Permission(PermissionHookData),
    /// Notification 事件数据
    Notification(NotificationHookData),
    /// InstructionsLoaded 事件数据
    InstructionsLoaded(InstructionsLoadedHookData),
    /// ConfigChange 事件数据
    ConfigChange(ConfigChangeHookData),
    /// Elicitation / ElicitationResult 事件数据
    Elicitation(ElicitationHookData),
    // ========== P3 事件数据 ==========
    /// UserPromptExpansion 事件数据
    UserPromptExpansion(UserPromptExpansionHookData),
    /// CwdChanged 事件数据
    CwdChanged(CwdChangedHookData),
    /// FileChanged 事件数据
    FileChanged(FileChangedHookData),
    /// TeammateIdle 事件数据
    TeammateIdle(TeammateIdleHookData),
}

impl HookData {
    /// 将事件数据转换为环境变量（用于传递给 hook 命令）
    pub fn to_env_vars(&self) -> Vec<(&'static str, String)> {
        match self {
            HookData::Tool(d) => vec![
                ("AEMEATH_TOOL_NAME", d.tool_name.clone()),
                (
                    "AEMEATH_TOOL_INPUT",
                    serde_json::to_string(&d.tool_input).unwrap_or_default(),
                ),
            ],
            HookData::Prompt(d) => vec![("AEMEATH_PROMPT", d.prompt.clone())],
            HookData::Stop(d) => vec![("AEMEATH_STOP_TURNS", d.turns.to_string())],
            HookData::Session(_) => vec![],
            HookData::Compact(d) => vec![
                ("AEMEATH_COMPACT_TURNS", d.turns.to_string()),
                (
                    "AEMEATH_COMPACT_MESSAGES_BEFORE",
                    d.messages_before.to_string(),
                ),
                (
                    "AEMEATH_COMPACT_MESSAGES_AFTER",
                    d.messages_after.map(|n| n.to_string()).unwrap_or_default(),
                ),
            ],
            HookData::Subagent(d) => {
                let mut vars = vec![
                    ("AEMEATH_SUBAGENT_PROMPT", d.prompt.clone()),
                    ("AEMEATH_SUBAGENT_SYSTEM", d.system.clone()),
                ];
                push_optional_env(&mut vars, "AEMEATH_SUBAGENT_MODEL_SPEC", &d.model_spec);
                push_optional_env(&mut vars, "AEMEATH_SUBAGENT_RESULT", &d.result);
                push_optional_display(&mut vars, "AEMEATH_SUBAGENT_TURNS", d.turns);
                push_optional_display(&mut vars, "AEMEATH_SUBAGENT_IS_ERROR", d.is_error);
                vars
            }
            // P2 事件
            HookData::Permission(d) => vec![
                ("AEMEATH_PERMISSION_TOOL_NAME", d.tool_name.clone()),
                ("AEMEATH_PERMISSION_RULE", d.permission_rule.clone()),
            ],
            HookData::Notification(d) => vec![
                ("AEMEATH_NOTIFICATION_TEXT", d.notification_text.clone()),
                ("AEMEATH_NOTIFICATION_TYPE", d.notification_type.clone()),
            ],
            HookData::InstructionsLoaded(d) => vec![
                ("AEMEATH_INSTRUCTIONS_FILE_PATH", d.file_path.clone()),
                ("AEMEATH_INSTRUCTIONS_TYPE", d.instruction_type.clone()),
            ],
            HookData::ConfigChange(d) => {
                let mut vars = vec![("AEMEATH_CONFIG_FILE", d.config_file.clone())];
                push_optional_env(&mut vars, "AEMEATH_CONFIG_CHANGED_FIELD", &d.changed_field);
                vars
            }
            HookData::Elicitation(d) => {
                let mut vars = vec![("AEMEATH_ELICITATION_SERVER", d.server_name.clone())];
                push_optional_env(&mut vars, "AEMEATH_ELICITATION_TEXT", &d.elicitation_text);
                push_optional_env(&mut vars, "AEMEATH_ELICITATION_RESPONSE", &d.user_response);
                vars
            }
            // P3 事件
            HookData::UserPromptExpansion(d) => vec![
                ("AEMEATH_PROMPT_ORIGINAL", d.original_input.clone()),
                ("AEMEATH_PROMPT_EXPANDED", d.expanded_input.clone()),
            ],
            HookData::CwdChanged(d) => vec![
                ("AEMEATH_CWD_OLD", d.old_cwd.clone()),
                ("AEMEATH_CWD_NEW", d.new_cwd.clone()),
            ],
            HookData::FileChanged(d) => vec![
                ("AEMEATH_FILE_PATH", d.file_path.clone()),
                ("AEMEATH_FILE_CHANGE_TYPE", d.change_type.clone()),
            ],
            HookData::TeammateIdle(d) => {
                let mut vars = vec![("AEMEATH_TEAMMATE_NAME", d.teammate_name.clone())];
                push_optional_env(&mut vars, "AEMEATH_TEAMMATE_IDLE_REASON", &d.idle_reason);
                vars
            }
        }
    }
}

/// 向环境变量列表推入可选值（值为 Option<String> 时）
fn push_optional_env(
    vars: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: &Option<String>,
) {
    if let Some(ref v) = value {
        vars.push((key, v.clone()));
    }
}

/// 向环境变量列表推入可选值（值为 Display trait 时）
fn push_optional_display<T: std::fmt::Display>(
    vars: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<T>,
) {
    if let Some(v) = value {
        vars.push((key, v.to_string()));
    }
}

/// 工具相关 hook 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolHookData {
    /// 工具名
    pub tool_name: String,
    /// 工具输入参数（JSON）
    pub tool_input: serde_json::Value,
    /// 工具执行结果（仅 PostToolUse）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<String>,
    /// 是否为错误结果（仅 PostToolUse）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// UserPrompt hook 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptHookData {
    /// 用户输入文本
    pub prompt: String,
}

/// Stop 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopHookData {
    /// agent 循环执行的轮次
    pub turns: usize,
}

/// SessionStart 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHookData {}

/// PreCompact / PostCompact 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactHookData {
    /// agent 循环执行的轮次
    pub turns: usize,
    /// 压缩前消息数量
    pub messages_before: usize,
    /// 压缩后消息数量（仅 PostCompact）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_after: Option<usize>,
    /// 是否实际执行了压缩
    pub was_compacted: bool,
}

/// SubagentStart / SubagentStop 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentHookData {
    /// sub-agent 的输入提示
    pub prompt: String,
    /// 系统消息
    pub system: String,
    /// 使用的模型规格（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_spec: Option<String>,
    /// 执行结果（仅 SubagentStop）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// 执行的轮次（仅 SubagentStop）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<usize>,
    /// 是否为错误结果（仅 SubagentStop）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

// ========== P2 事件数据 ==========

/// PermissionRequest / PermissionDenied 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionHookData {
    /// 工具名
    pub tool_name: String,
    /// 权限规则
    pub permission_rule: String,
}

/// Notification 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationHookData {
    /// 通知文本
    pub notification_text: String,
    /// 通知类型
    pub notification_type: String,
}

/// InstructionsLoaded 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionsLoadedHookData {
    /// 文件路径
    pub file_path: String,
    /// 指令类型（"claude_md" / "guidance"）
    pub instruction_type: String,
}

/// ConfigChange 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeHookData {
    /// 配置文件
    pub config_file: String,
    /// 变更的字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_field: Option<String>,
}

/// Elicitation / ElicitationResult 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationHookData {
    /// MCP 服务器名
    pub server_name: String,
    /// 请求文本（仅 Elicitation）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation_text: Option<String>,
    /// 用户响应（仅 ElicitationResult）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_response: Option<String>,
}

// ========== P3 事件数据 ==========

/// UserPromptExpansion 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptExpansionHookData {
    /// 原始用户输入
    pub original_input: String,
    /// 展开后的输入
    pub expanded_input: String,
}

/// CwdChanged 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdChangedHookData {
    /// 旧工作目录
    pub old_cwd: String,
    /// 新工作目录
    pub new_cwd: String,
}

/// FileChanged 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedHookData {
    /// 文件路径
    pub file_path: String,
    /// 变更类型
    pub change_type: String,
}

/// TeammateIdle 事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateIdleHookData {
    /// 队友名称
    pub teammate_name: String,
    /// 空闲原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_reason: Option<String>,
}
