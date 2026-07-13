//! 压缩后消息重组
//!
//! 清理孤立工具调用对（ToolUse/ToolResult 配对修复）。
//!
//! ## 模块结构
//! - `sanitize_pairs` — ToolUse/ToolResult 配对修复

mod sanitize_pairs;

pub use sanitize_pairs::sanitize_tool_pairs;

#[cfg(test)]
#[path = "restore/tests.rs"]
mod tests;
