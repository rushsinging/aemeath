//! Published language for the tools feature.
//!
//! This module exposes tool-domain DTOs and shared-kernel tool types without
//! exposing tool execution internals.

pub mod agent_port;
pub mod context;
pub mod memory_source;
pub mod ports;
pub mod published_language;
pub mod resources;
pub mod schema_validator;
#[cfg(test)]
mod schema_validator_tests;
pub mod scope_profile;
pub mod shell_safety;
pub mod skill_pl;
pub mod skill_ports;
pub mod suspension;
#[cfg(test)]
pub(crate) mod test_support;
pub mod tool;
pub mod tool_types;
pub mod types;

#[cfg(test)]
mod scope_profile_tests;
#[cfg(test)]
mod skill_pl_tests;

pub use agent_port::{AgentDispatch, AgentRunRequest, AgentRunTerminal, AgentRunner};
pub use context::{
    AuthorizationContext, CancellationSignal, ExecutionScope, ExecutionScopeBuilder, FixedGuidance,
    FixedPlanMode, Guidance, InvocationSource, MutexReadSet, PlanModeState, ProgressSink, ReadSet,
    ToolExecutionContext, ToolExecutionPorts, WorkspaceReadAccess,
};
pub use memory_source::MemoryPortSource;
pub use ports::{
    ToolCatalogPort, ToolExecutionContextBindingGuard, ToolExecutionContextBindingPort,
    ToolExecutionPort,
};
pub use published_language::{
    CancellationDeclaration, ConcurrencyDeclaration, InputSafetyDeclaration, RegistryScopeName,
    ToolCapabilities, ToolCapability, ToolCatalogSnapshot, ToolDescriptor, ToolErrorKind,
    ToolInvocation, ToolName, ToolOutcome as ToolExecutionOutcome, ToolProfileName,
};
pub use resources::CatalogQuery;
pub use scope_profile::{ProfileExpansionError, ToolProfile};
// 独立 Skill Published Language + 双端口（Issue #912）。
pub use skill_pl::{
    CacheHint, PromptFragment, SkillDescriptor, SkillError, SkillMaterializationQuery,
    SkillMaterializationRevision, SkillMaterializationSnapshot, SkillQuery, SkillSource,
    SkillSourceKind,
};
pub use skill_ports::{SkillCatalogPort, SkillMaterializationPort};
pub use suspension::{ToolSuspension, UserInteractionSpec, UserOption, UserQuestion};
pub use tool::{Tool, ToolListProvider, TypedTool, TypedToolAdapter, TypedToolResult};
pub use tool_types::{
    AgentProgressEvent, AgentProgressKind, AgentToolCallProgress, ImageData, SessionReminder,
    SessionReminders, ToolOutcome, ToolResult,
};
