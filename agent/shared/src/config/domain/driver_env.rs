//! Driver name → API key env var mapping.
//!
//! Single source of truth for driver→env name resolution.
//! Consumers (provider_client.rs, pool.rs) MUST reference this, NEVER duplicate.

/// Resolve the driver-specific API key environment variable name.
///
/// Accepts a driver name string (e.g. "anthropic", "openai") to avoid
/// depending on `ProviderDriverKind` from the provider feature.
///
/// Returns `None` for drivers that don't have a driver-specific env var
/// (Zhipu, LiteLLM).
pub fn driver_api_key_env_name(driver: &str) -> Option<&'static str> {
    match driver.to_ascii_lowercase().as_str() {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "volcengine" => Some("VOLCENGINE_CODING_PLAN_API_KEY"),
        "minimax" => Some("MINIMAX_API_KEY"),
        "mimo" => Some("MIMO_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "agnes" => Some("AGNES_API_KEY"),
        "ollama" => Some("OLLAMA_API_KEY"),
        // Zhipu and LiteLLM don't have driver-specific env vars
        "zhipu" | "litellm" => None,
        _ => None,
    }
}

/// Fallback generic env var names for API key resolution, in priority order.
pub fn fallback_api_key_env_names() -> &'static [&'static str] {
    &["LLM_API_KEY", "OPENAI_API_KEY"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_drivers() {
        assert_eq!(
            driver_api_key_env_name("anthropic"),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(
            driver_api_key_env_name("Anthropic"),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(
            driver_api_key_env_name("ANTHROPIC"),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(driver_api_key_env_name("openai"), Some("OPENAI_API_KEY"));
        assert_eq!(
            driver_api_key_env_name("volcengine"),
            Some("VOLCENGINE_CODING_PLAN_API_KEY")
        );
        assert_eq!(driver_api_key_env_name("minimax"), Some("MINIMAX_API_KEY"));
        assert_eq!(driver_api_key_env_name("mimo"), Some("MIMO_API_KEY"));
        assert_eq!(
            driver_api_key_env_name("deepseek"),
            Some("DEEPSEEK_API_KEY")
        );
        assert_eq!(driver_api_key_env_name("agnes"), Some("AGNES_API_KEY"));
        assert_eq!(driver_api_key_env_name("ollama"), Some("OLLAMA_API_KEY"));
    }

    #[test]
    fn test_drivers_without_specific_env() {
        assert_eq!(driver_api_key_env_name("zhipu"), None);
        assert_eq!(driver_api_key_env_name("litellm"), None);
    }

    #[test]
    fn test_unknown_driver() {
        assert_eq!(driver_api_key_env_name("unknown"), None);
    }
}
