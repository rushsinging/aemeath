use super::*;
use share::config::models::ProviderModelsConfig;
use share::config::{AgentsConfig, Config, ModelsConfig};
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
    let workspace = project::wire_production_workspace(std::env::temp_dir(), None)
        .expect("wire test workspace");

    let skill_wiring = tools::composition::wire_skills();
    let skill_catalog = skill_wiring.catalog();
    let tool_ports = tools::composition::TestCatalogExecutionFactory::empty();
    let runner = wire_agent_runner(
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        Arc::new(crate::application::run::active_registry::wire_active_run_registry()),
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
                    ) -> Result<
                        Vec<memory::api::reflection::ReflectionSafeSummary>,
                        memory::api::MemoryError,
                    > {
                        Ok(vec![])
                    }
                }
                #[async_trait::async_trait]
                impl memory::api::ReflectionHistoryStore for FakeRefl {
                    async fn append(
                        &self,
                        _record: &memory::api::reflection::ReflectionRecord,
                    ) -> Result<(), memory::api::MemoryError> {
                        Ok(())
                    }
                    async fn upsert(
                        &self,
                        _record: &memory::api::reflection::ReflectionRecord,
                    ) -> Result<(), memory::api::MemoryError> {
                        Ok(())
                    }
                }
                Arc::new(FakeRefl)
            };
            let hooks: Arc<dyn hook::HookDispatcher> = {
                struct FakeHook;
                #[async_trait::async_trait]
                impl hook::HookDispatcher for FakeHook {
                    async fn dispatch(
                        &self,
                        _invocation: hook::HookInvocationData,
                        _cancellation: &dyn hook::HookCancellationSignal,
                    ) -> hook::HookOutcomeData {
                        hook::HookOutcomeData::proceed()
                    }
                }
                Arc::new(FakeHook)
            };
            crate::application::run::context_factory::RuntimeContextFactory::new(
                tool_ports.catalog_port(),
                tool_ports.execution(),
                policy::allow_all(),
                refl,
                crate::application::run::test_task_access(),
                hooks,
                Arc::new(crate::ports::UnavailableUsageSink),
                None,
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
    agents.names.insert(
        "reviewer-glm".to_string(),
        share::config::AgentInstanceConfig {
            role: "reviewer".to_string(),
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
