use hook::api::HookRunner;
use share::config::{AgentRoleConfig, AgentsConfig, ModelsConfig};
use std::sync::Arc;

use crate::ports::ProviderFactory;

mod finalize;
pub use finalize::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
mod logging;
mod loop_helpers;
mod loop_run;
pub(crate) mod progress;
mod setup;
#[cfg(test)]
mod tests;

pub struct CliAgentRunner {
    /// Provider factory for building sub-agent bindings from model specs.
    pub factory: Arc<dyn ProviderFactory>,
    /// Shared per-Run registry used by Main and every Sub Run.
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    /// Agent config for role resolution.
    pub agents_config: Arc<AgentsConfig>,
    /// Hook runner for executing sub-agent hooks.
    pub hook_runner: HookRunner,
    /// Default reasoning setting for sub-agents (from config / CLI).
    pub reasoning: bool,
    /// Model entries config for reasoning lookup and ProviderBuildSpec construction.
    pub models_config: Arc<ModelsConfig>,
    /// Committed configuration snapshot frozen for sub-run prompt materialization.
    pub config_snapshot: share::config::domain::snapshot::ConfigSnapshot,
    /// Language frozen with the configuration snapshot.
    pub language: String,
    /// Per-request API timeout (seconds) forwarded to ProviderBuildSpec.
    pub api_timeout_secs: u64,
    pub max_tool_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    /// Runtime-owned workspace source used to derive isolated sub-run views.
    pub workspace: crate::application::tool_execution_adapters::RuntimeWorkspaceAccess,
    pub tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    pub tool_execution: Arc<dyn tools::ToolExecutionPort>,
    pub tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    /// Skill materializer shared with sub-run isolated contexts so that
    /// sub-agents materialize the configured skill set into their prompt.
    pub skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    pub policy: Arc<dyn policy::PolicyPort>,
}

impl CliAgentRunner {
    /// Resolve the required role to its configuration.
    fn resolve_role(&self, role: &str) -> Option<&AgentRoleConfig> {
        self.agents_config.roles.get(role)
    }

    fn role_max_tokens_override(role: &AgentRoleConfig) -> Option<u32> {
        role.max_tokens.filter(|tokens| *tokens > 0)
    }
}
