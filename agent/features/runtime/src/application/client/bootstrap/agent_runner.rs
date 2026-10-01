use crate::application::run::context::ParentRunContextSource;
use crate::application::run::derived as agent_runner;
use crate::ports::ProviderFactory;
#[cfg(test)]
use share::config::AgentsConfig;
#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct AgentRunnerAssemblyData {
    pub runner: Arc<dyn tools::published::agent::AgentRunner>,
    pub parent_context_source: ParentRunContextSource,
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub runtime_context_factory:
        Arc<crate::application::run::context_factory::RuntimeContextFactory>,
}

#[allow(clippy::too_many_arguments)]
pub fn wire_agent_runner(
    factory: Arc<dyn ProviderFactory>,
    active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    tool_result_materializer: Arc<
        crate::application::tool::tool_result_materializer::ToolResultMaterializer,
    >,
    workspace: project::Workspace,
    skill_catalog: Arc<dyn tools::published::skill::SkillCatalogPort>,
    parent_context_source: ParentRunContextSource,
    runtime_context_factory: Arc<crate::application::run::context_factory::RuntimeContextFactory>,
) -> AgentRunnerAssemblyData {
    let parent_context_for_runner = parent_context_source.clone();
    let active_run_for_runner = active_run.clone();
    let semaphore_for_runner = agent_semaphore.clone();
    let factory_for_runner = runtime_context_factory.clone();
    let runner: Arc<dyn tools::published::agent::AgentRunner> =
        Arc::new(agent_runner::CliAgentRunner {
            factory,
            active_run: active_run_for_runner,
            max_tool_concurrency,
            agent_semaphore: semaphore_for_runner,
            tool_result_materializer,
            workspace: crate::application::run::workspace::RuntimeWorkspaceAccess::new(workspace),
            skill_catalog,
            parent_context: parent_context_for_runner,
            runtime_context_factory: factory_for_runner,
        });
    AgentRunnerAssemblyData {
        runner,
        parent_context_source,
        active_run,
        max_tool_concurrency,
        max_agent_concurrency: agent_semaphore.available_permits(),
        agent_semaphore,
        runtime_context_factory,
    }
}

#[cfg(test)]
fn has_multi_provider_or_agent_roles(
    agents: Option<&AgentsConfig>,
    models_config: &share::config::ModelsConfig,
) -> bool {
    models_config.providers.len() > 1 || agents.map(|a| !a.names.is_empty()).unwrap_or(false)
}

/// Resolve the effective logs directory from config or an explicit fallback.
///
/// #1385: accepts explicit `agents_dir` so the caller (production composition)
/// threads one resolved `agents_dir` through every path; tests exercise the
/// same contract without calling `global_logs_dir()`.
#[cfg(test)]
fn resolve_role_logs_dir(
    config_file: Option<&share::config::domain::snapshot::ConfigSnapshot>,
    agents_dir: &Path,
) -> PathBuf {
    config_file
        .and_then(|config| config.logs_dir())
        .map(expand_tilde_path)
        .unwrap_or_else(|| agents_dir.join("logs"))
}

#[cfg(test)]
fn expand_tilde_path(path: &str) -> PathBuf {
    if path.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_default();
        PathBuf::from(path.replacen('~', &home.to_string_lossy(), 1))
    } else {
        PathBuf::from(path)
    }
}

impl AgentRunnerAssemblyData {
    /// 共享的 runtime context factory（Main/Derived 同源）。
    pub fn runtime_context_factory(
        &self,
    ) -> &Arc<crate::application::run::context_factory::RuntimeContextFactory> {
        &self.runtime_context_factory
    }
}

#[cfg(test)]
#[path = "agent_runner_tests.rs"]
mod tests;
