use super::{ConfigIntent, ConfigProvider};

#[test]
fn config_provider_updates_provider_model_context_size_and_thinking() {
    let mut config = ConfigProvider::default();

    config.apply(ConfigIntent::SetProviderModel {
        provider: Some("anthropic".to_string()),
        model_id: Some("claude-opus".to_string()),
    });
    config.apply(ConfigIntent::SetContextSize(200_000));
    config.apply(ConfigIntent::SetThinking(false));

    assert_eq!(config.provider(), Some("anthropic"));
    assert_eq!(config.model_id(), Some("claude-opus"));
    assert_eq!(config.context_size(), 200_000);
    assert!(!config.thinking());
}
