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
    audit, file_snapshot, hooks, legacy, logging, memory, models, permissions, scope, skills,
    storage, tools, ui, update,
};

// Re-exports for backward compatibility
pub use audit::AuditConfig;
pub use domain::config::{Config, GuidanceConfig, GuidanceReloadPolicy};
pub use file_snapshot::{FileChange, FileChangeKind, FileSnapshot};
pub use hooks::HooksConfig;
pub use legacy::{ApiConfig, ModelConfig};
pub use logging::LoggingConfig;
pub use memory::{MemoryConfig, ReflectionConfig};
pub use models::{ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
pub use permissions::{PermissionConfig, PermissionModeConfig};
pub use skills::SkillsConfig;
pub use storage::StorageConfig;
pub use tools::{AgentRoleConfig, AgentsConfig, ToolResultConfig, ToolsConfig};
pub use ui::{TaskLifecycleConfig, TaskListConfig, UiConfig};
pub use update::UpdateConfig;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.model.name, "claude-sonnet-4-6");
        assert_eq!(config.model.max_tokens, 8192);
        assert!(config.ui.markdown);
        assert!(config.storage.persist_sessions);
        assert!(config.memory.enabled);
        assert_eq!(config.memory.max_entries, 100);
    }
}
