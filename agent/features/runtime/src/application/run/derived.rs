use std::sync::Arc;

use crate::application::run::context::ParentRunContextSource;
use crate::application::run::context_factory::RuntimeContextFactory;
use crate::ports::ProviderFactory;

mod finalize;
pub use finalize::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
pub(crate) mod loop_run;
pub(crate) mod progress;
pub(super) mod setup;
#[cfg(test)]
mod tests;

pub struct CliAgentRunner {
    /// Provider factory for building sub-agent bindings from model specs.
    pub factory: Arc<dyn ProviderFactory>,
    /// Shared per-Run registry used by Main and every Sub Run.
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    pub max_tool_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub tool_result_materializer:
        Arc<crate::application::tool::result_materialization::ToolResultMaterializer>,
    /// Runtime-owned workspace source used to derive isolated sub-run views.
    pub workspace: crate::application::workspace::access::RuntimeWorkspaceAccess,
    /// Skill materializer shared with sub-run isolated contexts so that
    /// sub-agents materialize the configured skill set into their prompt.
    pub skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    /// #1385 Task 6: Injectable parent context source — set by the Main Run
    /// loop before tool execution so sub-agent runs can derive from it.
    pub parent_context: ParentRunContextSource,
    /// #1248 Task 3: RuntimeContextFactory — same instance as SessionRuntime's.
    /// Used for sub-run RuntimeContext assembly without a separate factory.
    pub runtime_context_factory: Arc<RuntimeContextFactory>,
}

impl CliAgentRunner {
    #[cfg(test)]
    fn role_max_tokens_override(role: &share::config::AgentRoleConfig) -> Option<u32> {
        role.max_tokens.filter(|tokens| *tokens > 0)
    }
}
