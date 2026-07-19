use crate::application::agent::runner as agent_runner;
#[cfg(test)]
use crate::application::startup::config_paths;
use hook::api::HookRunner;
use provider::LlmClient;
use share::config::hooks::HooksConfig;
use share::config::{AgentsConfig, ModelsConfig};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;

pub fn build_hook_runner(hooks: Option<&HooksConfig>, _cwd: &Path) -> HookRunner {
    let runner = match hooks {
        Some(h) => HookRunner::new(h.clone()),
        None => HookRunner::empty(),
    };
    log::info!(target: crate::LOG_TARGET,
        "hook runner built: configured_events={}",
        runner.hook_count()
    );
    runner
}

pub fn start_session(resume_session_id: Option<String>) -> String {
    let session_id = resume_session_id.unwrap_or_else(context::session::new_session_id);
    log::info!(target: crate::LOG_TARGET, "session started");
    session_id
}

#[allow(clippy::too_many_arguments)]
pub fn build_agent_runner(
    models: Option<&ModelsConfig>,
    agents: Option<&AgentsConfig>,
    client: Arc<LlmClient>,
    hook_runner: HookRunner,
    reasoning: bool,
    timeout_secs: u64,
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
) -> Arc<agent_runner::CliAgentRunner> {
    let models_config = Arc::new(models.cloned().unwrap_or_default());
    let pool = build_llm_client_pool(agents, client.clone(), models_config.clone(), timeout_secs);
    let agents_config = Arc::new(agents.cloned().unwrap_or_default());

    Arc::new(agent_runner::CliAgentRunner {
        client,
        pool,
        active_run,
        agents_config,
        hook_runner,
        reasoning,
        models_config,
        max_tool_concurrency,
        agent_semaphore,
        tool_result_materializer,
        workspace: crate::application::tool_execution_adapters::RuntimeWorkspaceAccess::new(
            workspace,
        ),
        tool_catalog,
        tool_execution,
        tool_context_binding,
        skill_materializer: tools::composition::wire_skill_materialization(),
        policy,
    })
}

fn build_llm_client_pool(
    agents: Option<&AgentsConfig>,
    client: Arc<LlmClient>,
    models_config: Arc<share::config::ModelsConfig>,
    timeout_secs: u64,
) -> Option<Arc<provider::LlmClientPool>> {
    if !has_multi_provider_or_agent_roles(agents, &models_config) {
        return None;
    }

    Some(Arc::new(provider::LlmClientPool::new(
        client,
        models_config,
        timeout_secs,
    )))
}

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
    fn test_build_hook_runner_accepts_empty_config() {
        let hook_runner = build_hook_runner(None, Path::new("."));

        assert_eq!(hook_runner.hook_count(), 0);
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

        assert_eq!(hook_runner.hook_count(), 1);
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
