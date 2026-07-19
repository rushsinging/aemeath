//! Hook 执行结果与 JSON 输出

use serde::{Deserialize, Serialize};

/// hook 执行结果
#[derive(Debug, Clone)]
pub struct HookResult {
    /// hook 是否阻止了操作（任意非零退出码）
    pub blocked: bool,
    /// hook 的 stdout 输出（超限时为截断后的前缀，**NEVER** 因截断而清空）
    pub output: String,
    /// 如果 hook 执行失败，包含错误信息
    pub error: Option<String>,
    /// hook 进程退出码；进程未正常退出时为空。
    pub exit_code: Option<i32>,
    /// stdout/stderr 是否因超过 `DEFAULT_OUTPUT_LIMIT` 被截断。
    ///
    /// 截断是**信息性**事件，**NEVER** 影响阻断判定（`is_blocking`）也 **NEVER** 设 error
    /// ——hook 仍可能正常 exit 0。保留截断后的 stdout 前缀足以解析 JSON directive。
    /// 见 #1220。
    pub output_truncated: bool,
}

impl HookResult {
    /// 创建携带错误信息的 HookResult（无阻止，无输出）
    pub fn with_error(message: impl Into<String>) -> Self {
        Self {
            blocked: false,
            output: String::new(),
            error: Some(message.into()),
            exit_code: None,
            output_truncated: false,
        }
    }

    /// 从 output 字段解析 JSON 输出
    pub fn parse_json_output(&self) -> Option<HookJsonOutput> {
        if self.output.trim().is_empty() {
            return None;
        }
        serde_json::from_str::<HookJsonOutput>(&self.output).ok()
    }
}

/// Hook 的 JSON 输出（exit 0 时 stdout 可包含此 JSON）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookJsonOutput {
    /// 是否继续执行（false 时全局停止，需配合 stopReason）
    #[serde(default = "default_true")]
    pub r#continue: bool,
    /// 停止原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// 决策（"block" 表示阻止操作）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// 阻止原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 额外上下文（注入到 LLM 对话流）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    /// 系统消息（警告等，显示在 TUI）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    /// 事件特定输出（PreToolUse 用：permission/updatedInput 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// 统一的 hook 阻断判定。
///
/// 满足以下任一条件即视为阻断：
/// - `result.blocked`：hook 以非零退出码退出
/// - JSON `decision: "block"`
/// - JSON `continue: false`（Stop hook 语境下表示"不允许停止，须继续"）
///
/// 所有需要判定 hook 是否阻断的位置（finalize / hook_ui 等）都 **MUST** 调用本函数，
/// 避免判定条件分散导致不一致（参见 issue #372）。
pub fn is_blocking(result: &HookResult, json: &Option<HookJsonOutput>) -> bool {
    result.blocked
        || json
            .as_ref()
            .is_some_and(|j| j.decision.as_deref() == Some("block") || !j.r#continue)
}
