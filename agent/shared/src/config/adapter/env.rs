//! EnvAdapter — reads business environment variables into a ConfigPatch.
//!
//! This is the SINGLE place in the codebase that reads business env vars.
//! All other code MUST go through ConfigReader port to access configuration.

use crate::config::domain::merge::{
    AgentsConfigPatch, ApiConfigPatch, ConfigPatch, LoggingConfigPatch, ModelConfigPatch,
    PermissionConfigPatch, ToolsConfigPatch, UiConfigPatch,
};
use crate::config::permissions::PermissionModeConfig;

/// Read business environment variables and produce a ConfigPatch.
///
/// The returned patch only contains fields where an env var was set;
/// absent env vars yield `None` (no override).
pub fn read() -> ConfigPatch {
    let mut patch = ConfigPatch::default();

    // ── API ──────────────────────────────────────────────────

    let mut api = ApiConfigPatch::default();

    if let Ok(provider_str) = std::env::var("AEMEATH_PROVIDER") {
        api.provider = Some(provider_str);
    }

    if let Ok(key) = std::env::var("LLM_API_KEY") {
        api.key = Some(key);
    }

    if let Ok(url) = std::env::var("AEMEATH_BASE_URL") {
        api.base_url = Some(url);
    } else if let Ok(url) = std::env::var("LLM_BASE_URL") {
        api.base_url = Some(url);
    }

    if api.provider.is_some() || api.key.is_some() || api.base_url.is_some() {
        patch.api = Some(api);
    }

    // ── Model ────────────────────────────────────────────────

    let mut model = ModelConfigPatch::default();

    if let Ok(name) = std::env::var("AEMEATH_MODEL") {
        model.name = Some(name);
    }

    if let Ok(max_tokens) = std::env::var("AEMEATH_MAX_TOKENS") {
        if let Ok(val) = max_tokens.parse() {
            model.max_tokens = Some(val);
        }
    }

    if let Ok(context_size) = std::env::var("AEMEATH_CONTEXT_SIZE") {
        if let Ok(val) = context_size.parse() {
            model.context_size = Some(val);
        }
    }

    if model.name.is_some() || model.max_tokens.is_some() || model.context_size.is_some() {
        patch.model = Some(model);
    }

    // ── Permissions ──────────────────────────────────────────

    let mut perm = PermissionConfigPatch::default();

    if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
        match mode.to_lowercase().as_str() {
            "ask" => perm.mode = Some(PermissionModeConfig::Ask),
            "auto_read" | "autoread" => perm.mode = Some(PermissionModeConfig::AutoRead),
            "allow_all" | "allowall" | "auto_all" | "autoall" => {
                perm.mode = Some(PermissionModeConfig::AllowAll)
            }
            _ => {}
        }
    }

    if perm.mode.is_some() {
        patch.permissions = Some(perm);
    }

    // ── Tools / Agents ───────────────────────────────────────

    let mut tools = ToolsConfigPatch::default();

    if let Ok(val) = std::env::var("AEMEATH_MAX_TOOL_CONCURRENCY") {
        if let Ok(v) = val.parse::<usize>() {
            if v > 0 {
                tools.max_concurrency = Some(v);
            }
        }
    }

    if tools.max_concurrency.is_some() {
        patch.tools = Some(tools);
    }

    let mut agents = AgentsConfigPatch::default();

    if let Ok(val) = std::env::var("AEMEATH_MAX_AGENT_CONCURRENCY") {
        if let Ok(v) = val.parse::<usize>() {
            if v > 0 {
                agents.max_concurrency = Some(v);
            }
        }
    }

    if agents.max_concurrency.is_some() {
        patch.agents = Some(agents);
    }

    // ── UI ───────────────────────────────────────────────────

    let mut ui = UiConfigPatch::default();

    if std::env::var("AEMEATH_VERBOSE").is_ok() {
        ui.verbose = Some(true);
    }

    if std::env::var("NO_COLOR").is_ok() {
        ui.color = Some(false);
    }

    if ui.verbose.is_some() || ui.color.is_some() {
        patch.ui = Some(ui);
    }

    // ── Logging ──────────────────────────────────────────────

    let mut logging = LoggingConfigPatch::default();

    if let Ok(level) = std::env::var("AEMEATH_LOG_LEVEL") {
        logging.level = Some(level);
    }

    if logging.level.is_some() {
        patch.logging = Some(logging);
    }

    patch
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn remove_all_business_env() {
        for key in &[
            "AEMEATH_PROVIDER",
            "LLM_API_KEY",
            "AEMEATH_BASE_URL",
            "LLM_BASE_URL",
            "AEMEATH_MODEL",
            "AEMEATH_MAX_TOKENS",
            "AEMEATH_CONTEXT_SIZE",
            "AEMEATH_PERMISSION_MODE",
            "AEMEATH_MAX_TOOL_CONCURRENCY",
            "AEMEATH_MAX_AGENT_CONCURRENCY",
            "AEMEATH_VERBOSE",
            "NO_COLOR",
            "AEMEATH_LOG_LEVEL",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    #[serial]
    fn test_no_env_yields_empty_patch() {
        remove_all_business_env();
        let patch = read();
        assert!(patch.api.is_none());
        assert!(patch.model.is_none());
        assert!(patch.permissions.is_none());
        assert!(patch.tools.is_none());
        assert!(patch.agents.is_none());
        assert!(patch.ui.is_none());
        assert!(patch.logging.is_none());
    }

    #[test]
    #[serial]
    fn test_context_size() {
        remove_all_business_env();
        std::env::set_var("AEMEATH_CONTEXT_SIZE", "64000");
        let patch = read();
        assert_eq!(
            patch.model.as_ref().and_then(|m| m.context_size),
            Some(64000)
        );
        std::env::remove_var("AEMEATH_CONTEXT_SIZE");
    }

    #[test]
    #[serial]
    fn test_permission_mode() {
        remove_all_business_env();
        std::env::set_var("AEMEATH_PERMISSION_MODE", "allowAll");
        let patch = read();
        assert_eq!(
            patch.permissions.as_ref().and_then(|p| p.mode.clone()),
            Some(PermissionModeConfig::AllowAll)
        );
        std::env::remove_var("AEMEATH_PERMISSION_MODE");
    }

    #[test]
    #[serial]
    fn test_log_level() {
        remove_all_business_env();
        std::env::set_var("AEMEATH_LOG_LEVEL", "debug");
        let patch = read();
        assert_eq!(
            patch.logging.as_ref().and_then(|l| l.level.clone()),
            Some("debug".to_string())
        );
        std::env::remove_var("AEMEATH_LOG_LEVEL");
    }
}
