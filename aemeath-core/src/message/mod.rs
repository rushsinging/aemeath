//! 消息模型与完整性检查
//!
//! ## 模块结构
//! - `types` — 核心类型定义（Role, ContentBlock, Message, IntegrityIssue 等）
//! - `constructors` — Message 构造方法
//! - `query` — Message 内容查询方法
//! - `integrity` — 消息完整性检查与清理（sanitize, check, deep_clean）

mod types;
mod constructors;
mod query;
mod integrity;

// 类型定义
pub use types::*;
// 完整性检查函数
pub use integrity::{check_message_integrity, deep_clean_messages, sanitize_messages};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
