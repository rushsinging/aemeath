//! Configuration file management
//!
//! Supports layered configuration from multiple sources:
//! 1. Default values
//! 2. Global config file (`~/.agents/aemeath.json` by default)
//! 3. Project config file (`{cwd}/.agents/aemeath.json`)
//! 4. Environment variables
//! 5. Command line arguments

pub mod adapters;
pub mod domain;

pub use adapters::paths;
pub use domain::{
    audit, context, file_snapshot, hooks, legacy, logging, memory, models, permissions, runtime,
    scope, scoring, skills, storage, tools, ui, update,
};

// Re-exports for backward compatibility
pub use audit::AuditConfig;
pub use context::ContextConfig;
pub use domain::config::{Config, GuidanceConfig, GuidanceReloadPolicy};
pub use file_snapshot::{FileChange, FileChangeKind, FileSnapshot};
pub use hooks::HooksConfig;
pub use legacy::{ApiConfig, ModelConfig};
pub use logging::LoggingConfig;
pub use memory::{MemoryConfig, ReflectionConfig};
pub use models::{ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
pub use permissions::{PermissionConfig, PermissionModeConfig};
pub use runtime::RuntimeConfig;
pub use scoring::ScoringConfig;
pub use skills::SkillsConfig;
pub use storage::StorageConfig;
pub use tools::{
    AgentInstanceConfig, AgentRoleDefinition, AgentsConfig, ResolveAgentOutcome, ResolvedAgent,
    RolePolicyConfig, ToolResultConfig, ToolSelection, ToolsConfig,
};
pub use ui::{
    ElementSpacingOverride, MarkdownSpacingMode, MarkdownSpacingOverrides, SpacingLines,
    TaskLifecycleConfig, TaskListConfig, UiConfig,
};
pub use update::UpdateConfig;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
