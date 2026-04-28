//! Hook 配置定义
//!
//! 参考 Claude Code hook 系统，支持在 aemeath 生命周期的关键事件点执行用户自定义 shell 命令。
//!
//! ## 事件类型（25 个）
//! - `PreToolUse` — 工具执行前（可阻止/修改输入）
//! - `PostToolUse` — 工具执行后（可注入上下文）
//! - `PostToolUseFailure` — 工具执行失败后（可注入修复指导）
//! - `UserPromptSubmit` — 用户输入处理前（可修改/拒绝）
//! - `Stop` — agent 循环结束（可阻止停止）
//! - `StopFailure` — API 错误导致结束（观察性）
//! - `SessionStart` — 会话开始（注入上下文）
//! - `SessionEnd` — 会话结束（发送消息/清理）
//! - `PreCompact` — 上下文压缩前（可阻止）
//! - `PostToolBatch` — 批量工具后汇总
//! - `SubagentStart` / `SubagentStop` — Sub-agent 生命周期
//! - `TaskCreated` / `TaskCompleted` — 任务生命周期
//! - P2: `PermissionRequest` / `PermissionDenied` / `Notification` / `InstructionsLoaded` / `ConfigChange` / `Elicitation` / `ElicitationResult`
//! - P3: `UserPromptExpansion` / `CwdChanged` / `FileChanged` / `TeammateIdle`
//!
//! ## 配置格式（在 config.json 中）
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       { "matcher": "Bash", "command": "echo 'about to run bash'" }
//!     ],
//!     "PostToolUse": [
//!       { "matcher": "", "command": "notify-send 'tool done'" }
//!     ]
//!   }
//! }
//! ```
//!
//! ## Exit Code + JSON 输出协议
//!
//! exit 0 = 成功。stdout 可包含 JSON（字段见 HookJsonOutput）
//! exit 2 = 阻止操作。stderr 作为反馈传给 LLM
//! exit 其他 = 非阻塞错误
//!
//! exit 0 时 JSON 支持的字段：
//! - `continue: false` + `stopReason` — 全局停止
//! - `decision: "block"` + `reason` — 阻止操作
//! - `additionalContext` — 注入额外上下文到 LLM 对话
//! - `systemMessage` — 系统警告消息
//! - `hookSpecificOutput` — 事件特定控制（PreToolUse 用）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Hook 事件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    /// 工具执行前触发（可阻止/修改输入）
    PreToolUse,
    /// 工具执行后触发（可注入上下文）
    PostToolUse,
    /// 工具执行失败后触发（可注入修复指导）
    PostToolUseFailure,
    /// 用户提交输入时触发（可修改/拒绝 prompt）
    UserPromptSubmit,
    /// Agent 循环结束时触发（可阻止停止）
    Stop,
    /// API 错误导致结束（观察性事件）
    StopFailure,
    /// 会话开始时触发（可注入上下文）
    SessionStart,
    /// 会话结束时触发（发送消息/清理）
    SessionEnd,
    /// 上下文压缩前触发（可阻止压缩）
    PreCompact,
    /// 上下文压缩后触发（可注入上下文/发送消息）
    PostCompact,
    /// 批量工具调用完成后触发（汇总注入上下文）
    PostToolBatch,
    /// Sub-agent 启动时触发
    SubagentStart,
    /// Sub-agent 结束时触发
    SubagentStop,
    /// TaskCreate 工具执行成功后触发（可注入上下文）
    TaskCreated,
    /// TaskUpdate 将任务标记为 completed 时触发（可注入上下文）
    TaskCompleted,
    // ========== P2 事件 ==========
    /// 权限审批弹窗前触发（可阻止/修改输入）
    PermissionRequest,
    /// 自动模式拒绝时触发（观察性）
    PermissionDenied,
    /// Claude 发送通知时触发（可注入上下文）
    Notification,
    /// CLAUDE.md / guidance 加载到上下文时触发（可注入上下文）
    InstructionsLoaded,
    /// 会话中配置文件变更时触发（观察性）
    ConfigChange,
    /// MCP 服务器请求用户输入前触发（可阻止/修改输入）
    Elicitation,
    /// 用户响应 MCP elicitation 后触发（可注入上下文）
    ElicitationResult,
    // ========== P3 事件 ==========
    /// 用户输入展开为提示时触发（可修改/拒绝）
    UserPromptExpansion,
    /// 工作目录改变时触发（观察性）
    CwdChanged,
    /// 监视文件在磁盘改变时触发（观察性）
    FileChanged,
    /// 团队队友空闲时触发（观察性）
    TeammateIdle,
}

/// 单个 hook 条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// 工具名匹配模式（空字符串匹配所有工具）
    /// 对于 PreToolUse/PostToolUse/PostToolUseFailure 有效，对其他事件忽略
    #[serde(default)]
    pub matcher: String,

    /// 要执行的 shell 命令
    pub command: String,

    /// 超时（秒），默认 60
    #[serde(default = "default_timeout_secs")]
    pub timeout: u64,
}

fn default_timeout_secs() -> u64 {
    60
}

/// 所有 hook 配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// 按事件类型分组的 hook 列表
    #[serde(flatten)]
    pub events: HashMap<HookEvent, Vec<HookEntry>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hooks_config_deserialize() {
        let json = r#"{
            "PreToolUse": [
                { "matcher": "Bash", "command": "echo bash-hook" }
            ],
            "PostToolUse": [
                { "matcher": "", "command": "notify-send done" }
            ],
            "Stop": [],
            "UserPromptSubmit": []
        }"#;
        let config: HooksConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.events.len(), 4);
        let pre = config.events.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[0].command, "echo bash-hook");
        assert_eq!(pre[0].timeout, 60);
    }

    #[test]
    fn test_hooks_config_default() {
        let config = HooksConfig::default();
        assert!(config.events.is_empty());
    }

    #[test]
    fn test_hooks_config_custom_timeout() {
        let json = r#"{
            "PreToolUse": [
                { "matcher": "", "command": "sleep 1", "timeout": 60 }
            ]
        }"#;
        let config: HooksConfig = serde_json::from_str(json).unwrap();
        let pre = config.events.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(pre[0].timeout, 60);
    }

    #[test]
    fn test_hook_event_serde_roundtrip() {
        let event = HookEvent::PreToolUse;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, "\"PreToolUse\"");
        let back: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn test_hook_event_deserialize_user_prompt_submit() {
        let json = r#"{
            "UserPromptSubmit": [
                { "matcher": "", "command": "echo validate" }
            ]
        }"#;
        let config: HooksConfig = serde_json::from_str(json).unwrap();
        assert!(config.events.contains_key(&HookEvent::UserPromptSubmit));
    }

    #[test]
    fn test_hook_event_deserialize_post_tool_use_failure() {
        let json = r#"{
            "PostToolUseFailure": [
                { "matcher": "Bash", "command": "echo failed" }
            ]
        }"#;
        let config: HooksConfig = serde_json::from_str(json).unwrap();
        assert!(config.events.contains_key(&HookEvent::PostToolUseFailure));
    }
}
