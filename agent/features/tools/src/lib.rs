#![deny(clippy::print_stdout, clippy::print_stderr)]

pub(crate) const LOG_TARGET: &str = "aemeath:agent:tools";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;

/// Composition-only adapter construction. Concrete adapter and backing types
/// remain private; production business code consumes the returned ports.
pub mod composition {
    pub use crate::adapters::composition::{
        wire_builtin_catalog_execution, wire_commands, wire_skill_materialization, wire_skills,
        CatalogExecutionWiring, CommandWiring, SkillWiring,
    };
    #[cfg(feature = "test-harness")]
    pub use crate::adapters::composition::{
        CountingToolCatalogGateway, TestCatalogExecution, TestCatalogExecutionFactory,
    };
}

/// Published tool-domain DTO types (kept as a public module facade).
pub use domain::types;

// Published language: shared-kernel tool types, DTOs, and ports.
pub use domain::{
    AgentDispatch, AgentProgressEvent, AgentProgressKind, AgentRunRequest, AgentRunTerminal,
    AgentRunner, AgentToolCallProgress, ApplicationControlCommand, ApplicationControlTarget,
    AuthorizationContext, CacheHint, CancellationDeclaration, CancellationSignal, CatalogQuery,
    CommandArgumentSchema, CommandCatalogPort, CommandCompletion, CommandDescriptor,
    CommandMechanism, CommandName, CommandParseError, CommandRoute, CommandRouterPort,
    CommandTarget, ConcurrencyDeclaration, ExecutionScope, ExecutionScopeBuilder, FixedGuidance,
    FixedPlanMode, Guidance, ImageData, InputSafetyDeclaration, InvocationSource, MemoryPortSource,
    MutexReadSet, ParsedArguments, PlanModeState, ProfileExpansionError, ProgressSink,
    PromptCommand, PromptFragment, ReadSet, RegistryScopeName, SessionReminder, SessionReminders,
    SkillCatalogPort, SkillDescriptor, SkillError, SkillMaterializationPort,
    SkillMaterializationQuery, SkillMaterializationRevision, SkillMaterializationSnapshot,
    SkillQuery, SkillSource, SkillSourceKind, SlashInput, SnapshotQueryCommand,
    SnapshotQueryTarget, Tool, ToolCapabilities, ToolCapability, ToolCatalogPort,
    ToolCatalogSnapshot, ToolDescriptor, ToolErrorKind, ToolExecutionContext,
    ToolExecutionContextBindingGuard, ToolExecutionContextBindingPort, ToolExecutionOutcome,
    ToolExecutionPort, ToolExecutionPorts, ToolInvocation, ToolListProvider, ToolName, ToolOutcome,
    ToolProfile, ToolProfileName, ToolResult, ToolSuspension, TypedTool, TypedToolAdapter,
    TypedToolResult, UserInteractionSpec, UserOption, UserQuestion, WorkspaceReadAccess,
};

// Schema validator (moved from runtime).
pub use domain::schema_validator::{
    format_tool_input_error, strip_runtime_meta, validate_tool_input, ToolInputMismatch,
    RUNTIME_META_KEYS,
};

// Runtime's phase-peel seam delegates to this Tools-owned typed parser.
pub use adapters::ask_user::ask_user_suspension;

// Gateway/OHS: tool catalog and registration wiring.
pub use adapters::wiring::{
    is_readonly_command, wire_tools, DefaultToolCatalogGateway, McpConnectionManager,
    McpServerConfig, McpTool, McpToolDef, McpTransportKind, ToolCatalogGateway,
};
