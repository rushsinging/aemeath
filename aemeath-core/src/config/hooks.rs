//! Hook 配置定义
//!
//! 支持在 aemeath 生命周期的关键事件点执行用户自定义 shell 命令。
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
//!     ],
//!     "Stop": [],
//!     "UserPrompt": []
//!   }
//! }
//! ```
//!
//! ## 事件类型
//! - `PreToolUse` — 工具执行前（可阻止执行）
//! - `PostToolUse` — 工具执行后（可修改输出）
//! - `Stop` — agent 循环结束（只读通知）
//! - `UserPrompt` — 用户输入处理前（可修改/拒绝）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Hook 事件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    /// 工具执行前触发
    PreToolUse,
    /// 工具执行后触发
    PostToolUse,
    /// Agent 循环结束时触发
    Stop,
    /// 用户输入处理前触发
    UserPrompt,
}

/// 单个 hook 条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// 工具名匹配模式（空字符串匹配所有工具）
    /// 对于 PreToolUse/PostToolUse 有效，对其他事件忽略
    #[serde(default)]
    pub matcher: String,

    /// 要执行的 shell 命令
    pub command: String,

    /// 超时（秒），默认 30
    #[serde(default = "default_timeout_secs")]
    pub timeout: u64,
}

fn default_timeout_secs() -> u64 {
    30
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
            "UserPrompt": []
        }"#;
        let config: HooksConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.events.len(), 4);
        let pre = config.events.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[0].command, "echo bash-hook");
        assert_eq!(pre[0].timeout, 30);
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
}
