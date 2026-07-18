//! Published language for the tools feature.
//!
//! This module exposes tool-domain DTOs and shared-kernel tool types without
//! exposing tool execution internals.

pub mod agent_port;
pub mod context;
pub mod ports;
pub mod published_language;
pub mod resources;
pub mod scope_profile;
pub mod tool;
pub mod tool_types;
pub mod types;

#[cfg(test)]
mod scope_profile_tests;

pub use agent_port::{AgentRunRequest, AgentRunTerminal, AgentRunner};
pub use context::ToolExecutionContext;
pub use ports::{ToolCatalogPort, ToolExecutionPort};
pub use published_language::{
    RegistryScopeName, ToolCapabilities, ToolCapability, ToolCatalogSnapshot, ToolInvocation,
    ToolOutcome as ToolExecutionOutcome, ToolProfileName,
};
pub use resources::ToolResources;
pub use scope_profile::{ProfileExpansionError, ToolProfile};
pub use tool::{Tool, ToolListProvider, TypedTool, TypedToolAdapter, TypedToolResult};
pub use tool_types::{
    AgentProgressEvent, AgentProgressKind, AgentToolCallProgress, ImageData, PathAccess, PathKind,
    PolicyDecision, SessionReminder, SessionReminders, ToolOutcome, ToolResult,
};
