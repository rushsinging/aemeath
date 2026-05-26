//! 共享值类型。

use serde::{Deserialize, Serialize};

/// 成本信息（Atomic 读取，纳秒级）。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CostInfo {
    /// 输入 token 数。
    pub input_tokens: u64,
    /// 输出 token 数。
    pub output_tokens: u64,
    /// 估算费用（USD）。
    pub cost_usd: f64,
}

/// 权限确认请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPrompt {
    /// 工具名称。
    pub tool_name: String,
    /// 操作描述。
    pub description: String,
    /// 风险等级。
    pub risk_level: String,
}

/// 状态信息（用于 TUI status line）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusInfo {
    /// 状态文本。
    pub text: String,
    /// 进度百分比（0-100）。
    pub progress: Option<u8>,
}

/// 任务摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    /// 任务 ID。
    pub id: String,
    /// 任务标题。
    pub subject: String,
    /// 任务状态。
    pub status: String,
    /// 优先级。
    pub priority: String,
}
