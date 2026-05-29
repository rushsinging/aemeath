//! tools crate 的 Public API 门面（DDD §6.4.3）。
//!
//! 对外仅经此模块暴露 use case 实际消费的注册入口与少量类型，
//! 内部各 Tool 实现模块保持 crate-private。

pub use crate::{
    register_all_tools, register_all_tools_except_agent, register_subagent_tools, McpTool,
};

pub use crate::bash::is_readonly_command;
pub use crate::mcp::McpServerConfig;
pub use crate::mcp_manager::McpConnectionManager;
