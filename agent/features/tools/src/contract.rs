//! Published language for the tools feature.
//!
//! This module exposes tool-domain DTOs and shared-kernel tool types without
//! exposing tool execution internals.

pub mod agent_port;
pub mod context;
pub mod tool;

pub use agent_port::{AgentRunRequest, AgentRunner};
pub use context::ToolExecutionContext;
pub use share::tool::{AgentToolCallProgress, ImageData, ToolResult};
pub use tool::Tool;

pub use crate::business::mcp::{McpServerConfig, McpToolDef, McpTransportKind};
