use hook::api::HookRunner;
use provider::api::LlmClient;
use provider::api::LlmClientPool;
use provider::api::StreamHandler;
use share::config::{AgentRoleConfig, AgentsConfig, ModelsConfig};
use std::sync::Arc;

mod finalize;
pub use finalize::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
mod logging;
mod loop_helpers;
mod loop_run;
pub(crate) mod progress;
mod setup;
#[cfg(test)]
mod tests;

pub struct CliAgentRunner {
    /// Default LLM client (used when no model_spec is provided).
    pub client: Arc<LlmClient>,
    /// Client pool for multi-LLM routing. `None` if only one model is configured.
    pub pool: Option<Arc<LlmClientPool>>,
    /// Agent config for role resolution.
    pub agents_config: Arc<AgentsConfig>,
    /// Hook runner for executing sub-agent hooks.
    pub hook_runner: HookRunner,
    /// Default reasoning setting for sub-agents (from config / CLI).
    pub reasoning: bool,
    /// Model entries config for reasoning lookup.
    pub models_config: Arc<ModelsConfig>,
}

/// A no-op stream handler for sub-agents (output goes to result, not terminal)
struct SilentHandler;

impl StreamHandler for SilentHandler {
    fn on_text(&mut self, _text: &str) {}
    fn on_tool_use_start(&mut self, _name: &str, _provider_id: Option<&str>, _index: usize) {}
    fn on_error(&mut self, _error: &str) {}
}
impl CliAgentRunner {
    /// Resolve a model spec to a concrete `"provider/model_id"` string.
    ///
    /// The `model_spec` passed in is already resolved by AgentTool:
    ///   - If the user set `model="deepseek/deepseek-chat"`, that comes through directly.
    ///   - If the user set `role="coder"`, that comes through as the role name.
    ///   - If neither was set, it's `None`.
    ///
    /// Resolution order:
    /// 1. If `model_spec` matches a role name in `agents.roles` → use the role's `model` field.
    /// 2. If `model_spec` contains `/` → treat as `"provider/model_id"` directly.
    /// 3. If `model_spec` is `None` → use `agents.default_model` if set.
    fn resolve_model_spec(&self, model_spec: Option<&str>) -> Option<String> {
        match model_spec {
            Some(spec) => {
                if let Some(role) = self.agents_config.roles.get(spec) {
                    if !role.model.is_empty() {
                        return Some(role.model.clone());
                    }
                }
                Some(spec.to_string())
            }
            None => {
                if !self.agents_config.default_model.is_empty() {
                    return Some(self.agents_config.default_model.clone());
                }
                None
            }
        }
    }

    /// Get the resolved role config (if any) for a model spec.
    fn resolve_role(&self, model_spec: Option<&str>) -> Option<&AgentRoleConfig> {
        model_spec.and_then(|spec| self.agents_config.roles.get(spec))
    }

    fn role_max_tokens_override(role: Option<&AgentRoleConfig>) -> Option<u32> {
        role.and_then(|r| r.max_tokens).filter(|tokens| *tokens > 0)
    }
}
