#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;

/// Composition-only adapter construction. Concrete adapter and backing types
/// remain private; production business code consumes the returned ports.
mod constants;
pub(crate) use constants::LOG_TARGET;

pub mod composition;

/// Published tool-domain DTO types (kept as a public module facade).
pub mod published;
pub use domain::types;

// Published language: shared-kernel tool types, DTOs, and ports.
pub use domain::{
    AuthorizationContext, CommittedTaskChange, ExecutionScope, FixedGuidance, Guidance, ImageData,
    MemoryPortSource, MutexReadSet, RegistryScopeName, TaskChangeFact, Tool, ToolCapabilities,
    ToolCapability, ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolErrorKind,
    ToolName, ToolOutcome, ToolProfile, ToolProfileName, ToolResult, WorkspaceReadAccess,
};

// Role-policy compilation: config strings → narrowed ToolProfile.
pub use domain::role_policy::role_profile_name;

// Test-only context builder（`test-harness` feature；生产构建不启用）。
#[cfg(any(test, feature = "test-harness"))]
pub use domain::test_support;

// Runtime's phase-peel seam delegates to this Tools-owned typed parser.

// Adapter façade: only MCP protocol values and the read-only command classifier.
pub use adapters::mcp_tool::McpTool;
