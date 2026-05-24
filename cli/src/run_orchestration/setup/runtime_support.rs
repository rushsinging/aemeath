use crate::agent_runner;
use aemeath_core::config::Config;
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::{self, JsonLogger};
use aemeath_llm::client::LlmClient;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(super) fn build_json_logger(
    session_id: &str,
    config_file: Option<&Config>,
) -> Option<Arc<Mutex<JsonLogger>>> {
    if !config_file
        .map(|config| config.logging.role_logs_enabled)
        .unwrap_or(true)
    {
        return None;
    }

    let logs_dir = resolve_role_logs_dir(config_file);
    let logging_cfg = config_file
        .map(|config| &config.logging)
        .cloned()
        .unwrap_or_default();
    match JsonLogger::new(session_id, &logs_dir, &logging_cfg) {
        Ok(logger) => Some(Arc::new(Mutex::new(logger))),
        Err(error) => {
            log::warn!("无法创建分化日志: {}", error);
            None
        }
    }
}

pub(super) fn build_agent_runner(
    config_file: Option<&Config>,
    client: Arc<LlmClient>,
    hook_runner: HookRunner,
    reasoning: bool,
    json_logger: Option<Arc<Mutex<JsonLogger>>>,
) -> Arc<agent_runner::CliAgentRunner> {
    let models_config = Arc::new(
        config_file
            .map(|config| config.models.clone())
            .unwrap_or_default(),
    );
    let pool = build_llm_client_pool(config_file, client.clone(), models_config.clone());
    let agents_config = Arc::new(
        config_file
            .map(|config| config.agents.clone())
            .unwrap_or_default(),
    );

    Arc::new(agent_runner::CliAgentRunner {
        client,
        pool,
        agents_config,
        hook_runner,
        reasoning,
        models_config,
        json_logger,
    })
}

fn build_llm_client_pool(
    config_file: Option<&Config>,
    client: Arc<LlmClient>,
    models_config: Arc<aemeath_core::config::ModelsConfig>,
) -> Option<Arc<aemeath_llm::LlmClientPool>> {
    if !has_multi_provider_or_agent_roles(config_file, &models_config) {
        return None;
    }

    Some(Arc::new(aemeath_llm::LlmClientPool::new(
        client,
        models_config,
    )))
}

fn has_multi_provider_or_agent_roles(
    config_file: Option<&Config>,
    models_config: &aemeath_core::config::ModelsConfig,
) -> bool {
    models_config.providers.len() > 1
        || !config_file
            .map(|config| config.agents.roles.is_empty())
            .unwrap_or(true)
}

fn resolve_role_logs_dir(config_file: Option<&Config>) -> PathBuf {
    config_file
        .and_then(|config| config.logging.logs_dir.as_ref())
        .map(|dir| expand_tilde_path(dir))
        .unwrap_or_else(|| logging::log_dir().join("logs"))
}

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
    use aemeath_core::config::models::ProviderModelsConfig;
    use aemeath_core::config::{AgentRoleConfig, Config, LoggingConfig, ModelsConfig};
    use std::collections::HashMap;

    fn config_with_logging(role_logs_enabled: bool, logs_dir: Option<&str>) -> Config {
        Config {
            logging: LoggingConfig {
                role_logs_enabled,
                logs_dir: logs_dir.map(str::to_string),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_resolve_role_logs_dir_uses_config_path() {
        let config = config_with_logging(true, Some("custom-logs"));

        let result = resolve_role_logs_dir(Some(&config));

        assert_eq!(result, PathBuf::from("custom-logs"));
    }

    #[test]
    fn test_resolve_role_logs_dir_expands_tilde_path() {
        let config = config_with_logging(true, Some("~/custom-logs"));

        let result = resolve_role_logs_dir(Some(&config));

        assert!(!result.to_string_lossy().starts_with('~'));
        assert!(result.ends_with("custom-logs"));
    }

    #[test]
    fn test_resolve_role_logs_dir_uses_default_logs_dir_without_config() {
        let result = resolve_role_logs_dir(None);

        assert_eq!(result, logging::log_dir().join("logs"));
    }

    #[test]
    fn test_build_json_logger_returns_none_when_role_logs_disabled() {
        let config = config_with_logging(false, None);

        let result = build_json_logger("session-id", Some(&config));

        assert!(result.is_none());
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
        let mut config = Config::default();
        config.agents.roles.insert(
            "reviewer".to_string(),
            AgentRoleConfig {
                description: "reviews code".to_string(),
                model: "provider/model".to_string(),
                ..Default::default()
            },
        );

        let result = has_multi_provider_or_agent_roles(Some(&config), &ModelsConfig::default());

        assert!(result);
    }

    #[test]
    fn test_has_multi_provider_or_agent_roles_returns_false_for_single_provider_without_roles() {
        let config = Config::default();
        let models_config = models_config_with_provider_count(1);

        let result = has_multi_provider_or_agent_roles(Some(&config), &models_config);

        assert!(!result);
    }
}
