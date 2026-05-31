//! Published language for the tools feature.
//!
//! This module exposes tool-domain DTOs and shared-kernel tool types without
//! exposing tool execution internals.

pub use share::tool::{AgentToolCallProgress, ImageData, Tool, ToolContext, ToolResult};

pub use crate::business::mcp::{McpServerConfig, McpToolDef, McpTransportKind};
