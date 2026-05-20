use crate::agent_runner;
use aemeath_core::logging::{self, JsonLogger};
use aemeath_llm::client::LlmClient;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(super) fn build_json_logger(
    session_id: &str,
    config_file: Option<&aemeath_core::config::Config>,
) -> Option<Arc<Mutex<JsonLogger>>> {
    if !config_file
        .map(|c| c.logging.role_logs_enabled)
        .unwrap_or(true)
    {
        return None;
    }

    let logs_dir = config_file
        .and_then(|c| c.logging.logs_dir.as_ref())
        .map(|d| {
            if d.starts_with('~') {
                let home = dirs::home_dir().unwrap_or_default();
                PathBuf::from(d.replacen('~', &home.to_string_lossy(), 1))
            } else {
                PathBuf::from(d)
            }
        })
        .unwrap_or_else(|| logging::log_dir().join("logs"));
    let logging_cfg = config_file.map(|c| &c.logging).cloned().unwrap_or_default();
    match JsonLogger::new(session_id, &logs_dir, &logging_cfg) {
        Ok(jl) => Some(Arc::new(Mutex::new(jl))),
        Err(e) => {
            log::warn!("无法创建分化日志: {}", e);
            None
        }
    }
}

pub(super) fn build_agent_runner(
    config_file: Option<&aemeath_core::config::Config>,
    client: Arc<LlmClient>,
    hook_runner: aemeath_core::hook::HookRunner,
    reasoning: bool,
    json_logger: Option<Arc<Mutex<JsonLogger>>>,
) -> Arc<agent_runner::CliAgentRunner> {
    let models_config_arc = Arc::new(config_file.map(|c| c.models.clone()).unwrap_or_default());
    let has_multi_providers = models_config_arc.providers.len() > 1
        || !config_file
            .map(|c| c.agents.roles.is_empty())
            .unwrap_or(true);

    let pool = if has_multi_providers {
        Some(Arc::new(aemeath_llm::LlmClientPool::new(
            client.clone(),
            models_config_arc.clone(),
        )))
    } else {
        None
    };
    let agents_config = Arc::new(config_file.map(|c| c.agents.clone()).unwrap_or_default());

    Arc::new(agent_runner::CliAgentRunner {
        client: client.clone(),
        pool,
        agents_config,
        hook_runner,
        reasoning,
        models_config: models_config_arc,
        json_logger,
    })
}
