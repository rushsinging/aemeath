#![deny(clippy::print_stdout, clippy::print_stderr)]

pub(crate) const LOG_TARGET: &str = "aemeath:agent:tools";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;

/// Published tool-domain DTO types (kept as a public module facade).
pub use domain::types;

// Published language: shared-kernel tool types, DTOs, and ports.
pub use domain::{
    AgentDispatch, AgentProgressEvent, AgentProgressKind, AgentRunRequest, AgentRunTerminal,
    AgentRunner, AgentToolCallProgress, CancellationSignal, CatalogQuery, ExecutionScope,
    ExecutionScopeBuilder, FixedGuidance, FixedPlanMode, Guidance, ImageData, InvocationSource,
    MemoryPortSource, MutexReadSet, PlanModeState, PolicyDecision, ProfileExpansionError,
    ProgressSink, ReadSet, RegistryScopeName, SessionReminder, SessionReminders, Tool,
    ToolCapabilities, ToolCapability, ToolCatalogPort, ToolCatalogSnapshot, ToolExecutionContext,
    ToolExecutionOutcome, ToolExecutionPort, ToolExecutionPorts, ToolInvocation, ToolListProvider,
    ToolName, ToolOutcome, ToolProfile, ToolProfileName, ToolResult, TypedTool, TypedToolAdapter,
    TypedToolResult, WorkspaceReadAccess,
};

// Gateway/OHS: tool catalog and registration wiring.
pub use adapters::wiring::{
    is_readonly_command, register_all_tools, register_all_tools_except_agent,
    register_subagent_tools, wire_tools, DefaultToolCatalogGateway, McpConnectionManager,
    McpServerConfig, McpTool, McpToolDef, McpTransportKind, ToolCatalog, ToolCatalogGateway,
    ToolRegistry,
};
