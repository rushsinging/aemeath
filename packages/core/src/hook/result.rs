//! Hook 执行结果与 JSON 输出

use serde::{Deserialize, Serialize};

/// hook 执行结果
#[derive(Debug, Clone)]
pub struct HookResult {
    /// hook 是否阻止了操作（任意非零退出码）
    pub blocked: bool,
    /// hook 的 stdout 输出
    pub output: String,
    /// 如果 hook 执行失败，包含错误信息
    pub error: Option<String>,
}

impl HookResult {
    /// 创建携带错误信息的 HookResult（无阻止，无输出）
    pub fn with_error(message: impl Into<String>) -> Self {
        Self {
            blocked: false,
            output: String::new(),
            error: Some(message.into()),
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
