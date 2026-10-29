#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 实现密封基建（#2146 实现权治理）：crate 内共享一个 Sealed 标记。
/// `pub(crate)` 模块 + `pub` trait——crate 外不可命名（路径不可达），
/// 因此「实现权」编译期归属本 crate；seal 的 PL trait 绑定
/// `crate::sealed::Sealed` 作为超 trait。
pub(crate) mod sealed {
    pub trait Sealed {}
}

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
pub use domain::published_language::CleanupConfirmation;
pub use domain::tool_definition::ToolDefinition;
pub use domain::{
    AuthorizationContext, BackgroundProcessAccess, BackgroundProcessAccessSource,
    CommittedTaskChange, ExecutionScope, FixedGuidance, Guidance, ImageData, MemoryPortSource,
    MutexReadSet, RegistryScopeName, TaskChangeFact, Tool, ToolCapabilities, ToolCapability,
    ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolErrorKind, ToolName, ToolOutcome,
    ToolProfile, ToolProfileName, ToolResult, WorkspaceReadAccess,
};

// Role-policy compilation: config strings → narrowed ToolProfile.
pub use domain::role_policy::role_profile_name;

// Test-only context builder（`test-harness` feature；生产构建不启用）。
#[cfg(any(test, feature = "test-harness"))]
pub use domain::test_support;

// Runtime's phase-peel seam delegates to this Tools-owned typed parser.

// Adapter façade: only MCP protocol values and the read-only command classifier.
pub use adapters::mcp_tool::McpTool;
