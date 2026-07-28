use config::ConfigReader;
use std::sync::Arc;

use crate::application::runtime_context::ParentRunContextSource;
use crate::application::runtime_context_factory::RuntimeContextFactory;
use crate::ports::ProviderFactory;

mod finalize;
pub use finalize::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
mod loop_helpers;
mod loop_run;
pub(crate) mod progress;
pub(super) mod setup;
#[cfg(test)]
pub(crate) mod test_config_reader;
#[cfg(test)]
mod tests;

pub struct CliAgentRunner {
    /// Provider factory for building sub-agent bindings from model specs.
    pub factory: Arc<dyn ProviderFactory>,
    /// Shared per-Run registry used by Main and every Sub Run.
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    /// ConfigReader is kept for the `complete` method (agentless LLM call).
    pub config_reader: Arc<dyn ConfigReader>,
    pub max_tool_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    /// Runtime-owned workspace source used to derive isolated sub-run views.
    pub workspace: crate::application::workspace_access::RuntimeWorkspaceAccess,
    /// Skill catalog shared with sub-run isolated contexts. Sub-agents receive
    /// metadata only; bodies are loaded on demand by the registered Skill tool.
    pub skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    /// #1385 Task 6: Injectable parent context source — set by the Main Run
    /// loop before tool execution so sub-agent runs can derive from it.
    pub parent_context: ParentRunContextSource,
    /// #1248 Task 3: RuntimeContextFactory — same instance as MainSessionShell's.
    /// Used for sub-run RuntimeContext assembly without a separate factory.
    pub runtime_context_factory: Arc<RuntimeContextFactory>,
}

impl CliAgentRunner {
    fn role_max_tokens_override(role: &share::config::AgentRoleConfig) -> Option<u32> {
        role.max_tokens.filter(|tokens| *tokens > 0)
    }
}
