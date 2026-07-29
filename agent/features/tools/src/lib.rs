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
    AgentDispatch, AgentProgressEvent, AgentProgressKind, AgentRunRequest, AgentRunTerminal,
    AgentRunner, AgentToolCallProgress, ApplicationControlCommand, ApplicationControlTarget,
    AuthorizationContext, CancellationDeclaration, CancellationSignal, CatalogQuery,
    CleanupConfirmation, CommandArgumentSchema, CommandCatalogPort, CommandCompletion,
    CommandDescriptor, CommandMechanism, CommandName, CommandParseError, CommandRoute,
    CommandRouterPort, CommandTarget, ConcurrencyDeclaration, ExecutionScope,
    ExecutionScopeBuilder, FixedGuidance, FixedPlanMode, Guidance, ImageData,
    InputSafetyDeclaration, InvocationSource, LoadedSkill, MemoryPortSource, MutexReadSet,
    ParsedArguments, PlanModeState, ProfileExpansionError, ProgressSink, ReadSet,
    RegistryScopeName, SessionReminder, SessionReminders, SkillCatalogPort, SkillCatalogSnapshot,
    SkillDescriptor, SkillError, SkillLoadPort, SkillLoadQuery, SkillQuery, SkillQuerySnapshot,
    SkillRequestCommand, SkillSlashRoute, SkillSource, SkillSourceKind, SlashInput,
    SnapshotQueryCommand, SnapshotQueryTarget, Tool, ToolCapabilities, ToolCapability,
    ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolDescriptor, ToolErrorKind,
    ToolExecutionContext, ToolExecutionOutcome, ToolExecutionPort, ToolExecutionPorts,
    ToolInvocation, ToolListProvider, ToolName, ToolOutcome, ToolProfile, ToolProfileName,
    ToolResult, ToolSuspension, ToolTerminalDetails, TypedTool, TypedToolAdapter, TypedToolResult,
    UserInteractionSpec, UserOption, UserQuestion, WorkspaceReadAccess,
};

// Schema validator (moved from runtime).
pub use domain::schema_validator::{
    format_tool_input_error, strip_runtime_meta, validate_tool_input, ToolInputMismatch,
    RUNTIME_META_KEYS,
};

// Runtime's phase-peel seam delegates to this Tools-owned typed parser.
pub use adapters::ask_user::ask_user_suspension;

// Adapter façade: only MCP protocol values and the read-only command classifier.
pub use adapters::bash::is_readonly_command;
pub use adapters::mcp::{McpServerConfig, McpToolDef, McpTransportKind};
pub use adapters::mcp_manager::McpConnectionManager;
pub use adapters::mcp_tool::McpTool;
