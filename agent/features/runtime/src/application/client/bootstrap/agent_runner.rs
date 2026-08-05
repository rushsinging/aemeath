use crate::application::run::context::ParentRunContextSource;
use crate::application::run::derived as agent_runner;
use crate::ports::ProviderFactory;
#[cfg(test)]
use share::config::AgentsConfig;
#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct AgentRunnerAssembly {
    pub runner: Arc<dyn tools::AgentRunner>,
    pub parent_context_source: ParentRunContextSource,
    pub active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub runtime_context_factory:
        Arc<crate::application::run::context_factory::RuntimeContextFactory>,
}

#[allow(clippy::too_many_arguments)]
pub fn build_agent_runner(
    factory: Arc<dyn ProviderFactory>,
    active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    tool_result_materializer: Arc<
        crate::application::tool::tool_result_materializer::ToolResultMaterializer,
    >,
    workspace: project::WorkspaceViews,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    parent_context_source: ParentRunContextSource,
    runtime_context_factory: Arc<crate::application::run::context_factory::RuntimeContextFactory>,
) -> AgentRunnerAssembly {
    let parent_context_for_runner = parent_context_source.clone();
    let active_run_for_runner = active_run.clone();
    let semaphore_for_runner = agent_semaphore.clone();
    let factory_for_runner = runtime_context_factory.clone();
    let runner: Arc<dyn tools::AgentRunner> = Arc::new(agent_runner::CliAgentRunner {
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
    AgentRunnerAssembly {
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
    models_config.providers.len() > 1 || agents.map(|a| !a.roles.is_empty()).unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::models::ProviderModelsConfig;
    use share::config::{AgentRoleConfig, AgentsConfig, Config, ModelsConfig};
    use std::collections::HashMap;

    fn snapshot_with_logs_dir(
        logs_dir: Option<&str>,
    ) -> share::config::domain::snapshot::ConfigSnapshot {
        let mut config = Config::default();
        config.logging.logs_dir = logs_dir.map(str::to_string);
        share::config::domain::snapshot::ConfigSnapshot::new(config)
    }

    #[test]
    fn build_agent_runner_constructs_without_panic() {
        let workspace = project::wire_production_workspace(std::env::temp_dir())
            .expect("wire test workspace")
            .into_views();

        let skill_wiring = tools::composition::wire_skills();
        let skill_catalog = skill_wiring.catalog();
        let tool_ports = tools::composition::TestCatalogExecutionFactory::empty();
        let runner = build_agent_runner(
            Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
            Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default()),
            10,
            Arc::new(tokio::sync::Semaphore::new(4)),
            crate::application::tool::test_support::test_tool_result_materializer(),
            workspace,
            skill_catalog.clone(),
            ParentRunContextSource::new(),
            Arc::new({
                let refl: Arc<dyn memory::api::ReflectionHistoryStore> = {
                    struct FakeRefl;
                    #[async_trait::async_trait]
                    impl memory::api::ReflectionHistoryQuery for FakeRefl {
                        async fn list(
                            &self,
                            _limit: usize,
                        ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::MemoryError>
                        {
                            Ok(vec![])
                        }
                    }
                    #[async_trait::async_trait]
                    impl memory::api::ReflectionHistoryStore for FakeRefl {
                        async fn append(
                            &self,
                            _record: &memory::api::ReflectionRecord,
                        ) -> Result<(), memory::MemoryError> {
                            Ok(())
                        }
                        async fn upsert(
                            &self,
                            _record: &memory::api::ReflectionRecord,
                        ) -> Result<(), memory::MemoryError> {
                            Ok(())
                        }
                    }
                    Arc::new(FakeRefl)
                };
                let hooks: Arc<dyn hook::HookPort> = {
                    struct FakeHook;
                    #[async_trait::async_trait]
                    impl hook::HookPort for FakeHook {
                        async fn dispatch(
                            &self,
                            _invocation: hook::HookInvocation,
                            _cancellation: &dyn hook::CancellationSignal,
                        ) -> hook::HookOutcome {
                            hook::HookOutcome::proceed()
                        }
                    }
                    Arc::new(FakeHook)
                };
                crate::application::run::context_factory::RuntimeContextFactory::new(
                    tool_ports.catalog_port(),
                    tool_ports.execution(),
                    Arc::new(policy::AllowAllPolicy),
                    refl,
                    crate::application::run::test_task_access(),
                    hooks,
                )
            }),
        );

        // Runner 只保存执行 Derived Run 所需的依赖；静态 Runtime 服务统一
        // 由同一个 RuntimeContextFactory 提供。
        assert!(runner.parent_context_source.get().is_none());
    }

    #[test]
    fn test_resolve_role_logs_dir_uses_config_path() {
        let snapshot = snapshot_with_logs_dir(Some("custom-logs"));

        let result = resolve_role_logs_dir(Some(&snapshot), Path::new("/tmp/agents"));

        assert_eq!(result, PathBuf::from("custom-logs"));
    }

    #[test]
    fn test_resolve_role_logs_dir_expands_tilde_path() {
        let snapshot = snapshot_with_logs_dir(Some("~/custom-logs"));

        let result = resolve_role_logs_dir(Some(&snapshot), Path::new("/tmp/agents"));

        assert!(!result.to_string_lossy().starts_with('~'));
        assert!(result.ends_with("custom-logs"));
    }

    #[test]
    fn test_resolve_role_logs_dir_uses_default_logs_dir_without_config() {
        // #1385: explicit agents_dir threaded through — fallback is
        // agents_dir.join("logs"), not global_logs_dir().join("logs") (which
        // would produce agents_dir/logs/logs).
        let result = resolve_role_logs_dir(None, Path::new("/tmp/agents"));

        assert_eq!(result, PathBuf::from("/tmp/agents/logs"));
    }

    fn models_config_with_provider_count(count: usize) -> ModelsConfig {
        let mut providers = HashMap::new();
        for index in 0..count {
            providers.insert(format!("provider-{index}"), ProviderModelsConfig::default());
        }

        ModelsConfig {
            providers,
            ..Default::default()
        }
    }

    #[test]
    fn test_has_multi_provider_or_agent_roles_detects_multiple_providers() {
        let models_config = models_config_with_provider_count(2);

        let result = has_multi_provider_or_agent_roles(None, &models_config);

        assert!(result);
    }

    #[test]
    fn test_has_multi_provider_or_agent_roles_detects_agent_roles() {
        let mut agents = AgentsConfig::default();
        agents.roles.insert(
            "reviewer".to_string(),
            AgentRoleConfig {
                description: "reviews code".to_string(),
                model: "provider/model".to_string(),
                ..Default::default()
            },
        );

        let result = has_multi_provider_or_agent_roles(Some(&agents), &ModelsConfig::default());

        assert!(result);
    }

    #[test]
    fn test_has_multi_provider_or_agent_roles_returns_false_for_single_provider_without_roles() {
        let agents = AgentsConfig::default();
        let models_config = models_config_with_provider_count(1);

        let result = has_multi_provider_or_agent_roles(Some(&agents), &models_config);

        assert!(!result);
    }
}
