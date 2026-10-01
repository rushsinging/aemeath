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
