#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod api;

mod business;
mod core;
mod utils;

// Re-export McpTool for dynamic creation (consumed via tools::api).
pub use business::mcp_tool::McpTool;
// Re-export 注册编排入口（consumed via tools::api）。
pub use core::registry::{
    register_all_tools, register_all_tools_except_agent, register_subagent_tools,
};
