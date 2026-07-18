#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:tools";

mod adapters;
mod domain;

/// Published tool-domain DTO types (kept as a public module facade).
pub use domain::types;

// Published language: shared-kernel tool types, DTOs, and ports.
pub use domain::{
    AgentProgressEvent, AgentProgressKind, AgentRunRequest, AgentRunTerminal, AgentRunner,
    AgentToolCallProgress, ImageData, PolicyDecision, ProfileExpansionError, RegistryScopeName,
    SessionReminder, SessionReminders, Tool, ToolCapabilities, ToolCapability, ToolCatalogPort,
    ToolCatalogSnapshot, ToolExecutionContext, ToolExecutionOutcome, ToolExecutionPort,
    ToolInvocation, ToolListProvider, ToolName, ToolOutcome, ToolProfile, ToolProfileName,
    ToolResources, ToolResult, TypedTool, TypedToolAdapter, TypedToolResult,
};

// Gateway/OHS: tool catalog and registration wiring.
pub use adapters::wiring::{
    is_readonly_command, register_all_tools, register_all_tools_except_agent,
    register_subagent_tools, wire_tools, DefaultToolCatalogGateway, McpConnectionManager,
    McpServerConfig, McpTool, McpToolDef, McpTransportKind, ToolCatalog, ToolCatalogGateway,
    ToolRegistry,
};
