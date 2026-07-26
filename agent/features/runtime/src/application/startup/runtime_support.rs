use crate::application::runtime_context::ParentRunContextSource;
use crate::application::subagent::runner as agent_runner;
use crate::ports::ProviderFactory;
#[cfg(test)]
use share::config::AgentsConfig;
#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn start_session(resume_session_id: Option<String>) -> String {
    let session_id = resume_session_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    log::info!(target: crate::LOG_TARGET, "session started");
    session_id
}

#[allow(clippy::too_many_arguments)]
pub fn build_agent_runner(
    config_reader: Arc<dyn config::ConfigReader>,
    factory: Arc<dyn ProviderFactory>,
    active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    tool_result_materializer: Arc<
        crate::application::tool_result_materialization::ToolResultMaterializer,
    >,
    workspace: project::WorkspaceViews,
    skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
    parent_context_source: ParentRunContextSource,
    runtime_context_factory: Arc<
        crate::application::runtime_context_factory::RuntimeContextFactory,
    >,
) -> Arc<agent_runner::CliAgentRunner> {
    Arc::new(agent_runner::CliAgentRunner {
        factory,
        active_run,
        config_reader,
        max_tool_concurrency,
        agent_semaphore,
        tool_result_materializer,
        workspace: crate::application::workspace_access::RuntimeWorkspaceAccess::new(workspace),
        skill_materializer,
        parent_context: parent_context_source,
        runtime_context_factory,
    })
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
        let skill_materializer = skill_wiring.materializer();
        let tool_ports = tools::composition::TestCatalogExecutionFactory::empty();
        let snapshot = share::config::domain::snapshot::ConfigSnapshot::new(Config::default());
        let config_reader: Arc<dyn config::ConfigReader> = Arc::new(
            crate::application::subagent::runner::test_config_reader::FixedConfigReader::from_snapshot(
                snapshot,
            ),
        );
        let runner = build_agent_runner(
            config_reader,
            Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
            Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
            10,
            Arc::new(tokio::sync::Semaphore::new(4)),
            crate::application::testing::test_tool_result_materializer(),
            workspace,
            skill_materializer.clone(),
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
                            _cancellation: &tokio_util::sync::CancellationToken,
                        ) -> hook::HookOutcome {
                            hook::HookOutcome::proceed()
                        }
                    }
                    Arc::new(FakeHook)
                };
                crate::application::runtime_context_factory::RuntimeContextFactory::new(
                    tool_ports.catalog_port(),
                    tool_ports.execution(),
                    tool_ports.binding(),
                    Arc::new(policy::AllowAllPolicy),
                    refl,
                    crate::application::testing::test_task_access(),
                    hooks,
                )
            }),
        );

        // #1385: runner now only carries fields used by run_agent / complete;
        // policy / hook_runner / tool_catalog / tool_execution / tool_context_binding
        // are all accessed through derived.context at runtime.
        assert_eq!(runner.max_tool_concurrency, 10);
    }

    #[test]
    fn test_start_session_uses_resume_session_id() {
        let session_id = start_session(Some("resume-id".to_string()));

        assert_eq!(session_id, "resume-id");
    }

    #[test]
    fn test_start_session_generates_session_id_without_resume() {
        let session_id = start_session(None);

        assert!(!session_id.is_empty());
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
