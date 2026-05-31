//! 消息模型
//!
//! ## 模块结构
//! - `types` — 核心类型定义（Role, ContentBlock, Message, IntegrityIssue 等）
//! - `constructors` — Message 构造方法
//! - `query` — Message 内容查询方法

mod constructors;
mod query;
mod types;

// 类型定义
pub use types::*;

#[cfg(test)]
#[path = "message/tests.rs"]
mod tests;
