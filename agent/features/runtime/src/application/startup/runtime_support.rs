use crate::application::agent::runner as agent_runner;
#[cfg(test)]
use crate::application::startup::config_paths;
use crate::ports::ProviderFactory;
use hook::HookPort;
use share::config::hooks::HooksConfig;
#[cfg(test)]
use share::config::AgentsConfig;
use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;

pub fn build_hook_runner(hooks: Option<&HooksConfig>, _cwd: &Path) -> Arc<dyn HookPort> {
    let runner: Arc<dyn HookPort> = match hooks {
        Some(h) => {
            Arc::new(hook::build_dispatcher(h, HashMap::new()).expect("hook config 必须合法"))
        }
        None => Arc::new(
            hook::build_dispatcher(&HooksConfig::default(), HashMap::new())
                .expect("空 hook config 必须合法"),
        ),
    };
    log::info!(target: crate::LOG_TARGET,
        "hook runner built: configured_events={}",
        hooks.map(|h| h.events.len()).unwrap_or(0)
    );
    runner
}

pub fn start_session(resume_session_id: Option<String>) -> String {
    let session_id = resume_session_id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    log::info!(target: crate::LOG_TARGET, "session started");
    session_id
}

#[allow(clippy::too_many_arguments)]
pub fn build_agent_runner(
    config_reader: Arc<dyn config::ConfigReader>,
    factory: Arc<dyn ProviderFactory>,
    hook_runner: Arc<dyn HookPort>,
    reasoning: bool,
    active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    policy: Arc<dyn policy::PolicyPort>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    tool_result_materializer: Arc<
        crate::application::tool_result_materialization::ToolResultMaterializer,
    >,
    workspace: project::WorkspaceViews,
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    skill_materializer: Arc<dyn tools::SkillMaterializationPort>,
) -> Arc<agent_runner::CliAgentRunner> {
    Arc::new(agent_runner::CliAgentRunner {
        factory,
        active_run,
        config_reader,
        hook_runner,
        reasoning,
        max_tool_concurrency,
        agent_semaphore,
        tool_result_materializer,
        workspace: crate::application::tool_execution_adapters::RuntimeWorkspaceAccess::new(
            workspace,
        ),
        tool_catalog,
        tool_execution,
        tool_context_binding,
        skill_materializer,
        policy,
    })
}

#[cfg(test)]
fn has_multi_provider_or_agent_roles(
    agents: Option<&AgentsConfig>,
    models_config: &share::config::ModelsConfig,
) -> bool {
    models_config.providers.len() > 1 || agents.map(|a| !a.roles.is_empty()).unwrap_or(false)
}

#[cfg(test)]
fn resolve_role_logs_dir(
    config_file: Option<&share::config::domain::snapshot::ConfigSnapshot>,
) -> PathBuf {
    config_file
        .and_then(|config| config.logs_dir())
        .map(expand_tilde_path)
        .unwrap_or_else(|| config_paths::global_logs_dir().join("logs"))
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
    use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
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
    fn build_agent_runner_preserves_injected_policy_identity_for_sub_runs() {
        let policy: Arc<dyn policy::PolicyPort> = Arc::new(policy::AllowAllPolicy);
        let workspace = project::wire_production_workspace(std::env::temp_dir())
            .expect("wire test workspace")
            .into_views();

        let tools = tools::composition::TestCatalogExecutionFactory::empty();
        let skill_wiring = tools::composition::wire_skills();
        let skill_materializer = skill_wiring.materializer();
        let snapshot = share::config::domain::snapshot::ConfigSnapshot::new(Config::default());
        let config_reader =
            crate::application::agent::runner::test_config_reader::FixedConfigReader::new(snapshot);
        let runner = build_agent_runner(
            config_reader,
            Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
            Arc::new(hook::build_dispatcher(&HooksConfig::default(), HashMap::new()).unwrap()),
            false,
            Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
            policy.clone(),
            10,
            Arc::new(tokio::sync::Semaphore::new(4)),
            crate::application::testing::test_tool_result_materializer(),
            workspace,
            tools.catalog_port(),
            tools.execution(),
            tools.binding(),
            skill_materializer.clone(),
        );

        assert!(
            Arc::ptr_eq(&runner.skill_materializer, &skill_materializer),
            "Sub Run runner 必须复用 Composition 注入的同一 Skill materializer"
        );
        assert!(
            Arc::ptr_eq(&runner.policy, &policy),
            "Sub Run runner 必须保留 Composition 注入的同一 PolicyPort 实例"
        );
    }

    #[test]
    fn test_build_hook_runner_accepts_empty_config() {
        let hook_runner = build_hook_runner(None, Path::new("."));
        // hook_runner is an Arc<dyn HookPort> — the dispatcher handles empty configs.
        let _ = hook_runner;
    }

    #[test]
    fn test_build_hook_runner_uses_config_hooks() {
        let mut events = HashMap::new();
        events.insert(
            HookEvent::PreToolUse,
            vec![HookEntry {
                matcher: "Bash".to_string(),
                command: "true".to_string(),
                timeout: 60,
            }],
        );
        let hooks = HooksConfig { events };

        let hook_runner = build_hook_runner(Some(&hooks), Path::new("project-root"));
        // hook_runner is an Arc<dyn HookPort> — config is handled by the dispatcher.
        let _ = hook_runner;
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

        let result = resolve_role_logs_dir(Some(&snapshot));

        assert_eq!(result, PathBuf::from("custom-logs"));
    }

    #[test]
    fn test_resolve_role_logs_dir_expands_tilde_path() {
        let snapshot = snapshot_with_logs_dir(Some("~/custom-logs"));

        let result = resolve_role_logs_dir(Some(&snapshot));

        assert!(!result.to_string_lossy().starts_with('~'));
        assert!(result.ends_with("custom-logs"));
    }

    #[test]
    fn test_resolve_role_logs_dir_uses_default_logs_dir_without_config() {
        let result = resolve_role_logs_dir(None);

        assert_eq!(result, config_paths::global_logs_dir().join("logs"));
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
