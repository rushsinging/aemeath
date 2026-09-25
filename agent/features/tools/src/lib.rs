#![deny(clippy::print_stdout, clippy::print_stderr)]

pub(crate) const LOG_TARGET: &str = "aemeath:agent:tools";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;

/// Composition-only adapter construction. Concrete adapter and backing types
/// remain private; production business code consumes the returned ports.
pub mod composition {
    pub use crate::adapters::composition::{
        wire_builtin_catalog_execution, wire_commands, wire_skills, CatalogExecutionWiring,
        CommandWiring, SkillWiring,
    };
    #[cfg(feature = "test-harness")]
    pub use crate::adapters::composition::{TestCatalogExecution, TestCatalogExecutionFactory};
}

/// Published tool-domain DTO types (kept as a public module facade).
pub use domain::types;

// Published language: shared-kernel tool types, DTOs, and ports.
pub use domain::{
    AgentDispatch, AgentProgressEvent, AgentProgressKind, AgentProgressSourceContext,
    AgentRunRequest, AgentRunTerminal, AgentRunner, AgentToolCallProgress,
    ApplicationControlCommand, ApplicationControlTarget, AuthorizationContext,
    CancellationDeclaration, CancellationSignal, CleanupConfirmation, CommandArgumentSchema,
    CommandCatalogPort, CommandCompletion, CommandDescriptor, CommandMechanism, CommandName,
    CommandParseError, CommandRoute, CommandRouterPort, CommandTarget, CommittedTaskChange,
    ExecutionScope, FixedGuidance, FixedPlanMode, Guidance, ImageData, InvocationSource,
    MemoryPortSource, MutexReadSet, ParsedArguments, ProgressSink, RegistryScopeName,
    SessionReminder, SessionReminders, SkillCatalogPort, SkillCatalogSnapshot, SkillDescriptor,
    SkillError, SkillLoadDecision, SkillLoadMutation, SkillLoadPort, SkillLoadScope,
    SkillLoadStateError, SkillLoadStatePort, SkillQuery, SkillRequestCommand, SkillSource,
    SkillSourceKind, SlashInput, SnapshotQueryCommand, SnapshotQueryTarget, SubRunActivityEvent,
    SubRunActivityKind, SubRunIdentity, SubRunStartedEvent, SubRunTerminalOutcome, TaskChangeFact,
    Tool, ToolCapabilities, ToolCapability, ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot,
    ToolErrorKind, ToolExecutionContext, ToolExecutionOutcome, ToolExecutionPort,
    ToolExecutionPorts, ToolInvocation, ToolName, ToolOutcome, ToolProfile, ToolProfileName,
    ToolProgressEvent, ToolResult, ToolSuspension, TypedTool, TypedToolAdapter, TypedToolResult,
    UserQuestion, WorkspaceReadAccess,
};

// Schema validator (moved from runtime).
pub use domain::schema_validator::{
    format_tool_input_error, strip_runtime_meta, validate_tool_input,
};

// Role-policy compilation: config strings → narrowed ToolProfile.
pub use domain::role_policy::{role_profile_name, RolePolicyCompileError};

// Runtime's phase-peel seam delegates to this Tools-owned typed parser.
pub use adapters::ask_user::ask_user_suspension;

// Adapter façade: only MCP protocol values and the read-only command classifier.
pub use adapters::bash::is_readonly_command;
pub use adapters::mcp::McpTransportKind;
pub use adapters::mcp_manager::McpConnectionManager;
pub use adapters::mcp_tool::McpTool;
