use config::ConfigReader;
use hook::HookPort;
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
pub(crate) mod test_config_reader;
#[cfg(test)]
mod tests;

pub struct CliAgentRunner {
    /// Provider factory for building sub-agent bindings from model specs.
    pub factory: Arc<dyn ProviderFactory>,
    /// Shared per-Run registry used by Main and every Sub Run.
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    /// ConfigReader is queried once at each Subagent Run creation.
    pub config_reader: Arc<dyn ConfigReader>,
    /// Hook runner for executing sub-agent hooks.
    pub hook_runner: Arc<dyn HookPort>,
    /// Default reasoning setting for sub-agents (from config / CLI).
    pub reasoning: bool,
    pub max_tool_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    /// Runtime-owned workspace source used to derive isolated sub-run views.
    pub workspace: crate::adapters::tool_runtime::RuntimeWorkspaceAccess,
    pub tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    pub tool_execution: Arc<dyn tools::ToolExecutionPort>,
    pub tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    /// Skill materializer shared with sub-run isolated contexts so that
    /// sub-agents materialize the configured skill set into their prompt.
    pub skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    pub policy: Arc<dyn policy::PolicyPort>,
}

impl CliAgentRunner {
    fn role_max_tokens_override(role: &share::config::AgentRoleConfig) -> Option<u32> {
        role.max_tokens.filter(|tokens| *tokens > 0)
    }
}
