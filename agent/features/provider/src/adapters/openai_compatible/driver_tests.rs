use super::*;

#[test]
fn every_openai_compatible_driver_derives_maximum_from_capability() {
    for (kind, expected_maximum, expected_mapping) in [
        (
            ProviderDriverKind::OpenAI,
            ReasoningLevel::Max,
            ReasoningMappingKind::Effort,
        ),
        (
            ProviderDriverKind::Zhipu,
            ReasoningLevel::Max,
            ReasoningMappingKind::Effort,
        ),
        (
            ProviderDriverKind::LiteLLM,
            ReasoningLevel::Max,
            ReasoningMappingKind::Effort,
        ),
        (
            ProviderDriverKind::Volcengine,
            ReasoningLevel::Medium,
            ReasoningMappingKind::Effort,
        ),
        (
            ProviderDriverKind::Minimax,
            ReasoningLevel::Medium,
            ReasoningMappingKind::ThinkingToggle,
        ),
        (
            ProviderDriverKind::Mimo,
            ReasoningLevel::Medium,
            ReasoningMappingKind::ThinkingToggle,
        ),
        (
            ProviderDriverKind::DeepSeek,
            ReasoningLevel::Max,
            ReasoningMappingKind::Effort,
        ),
        (
            ProviderDriverKind::Agnes,
            ReasoningLevel::Medium,
            ReasoningMappingKind::ThinkingToggle,
        ),
    ] {
        let driver = driver_for_provider_driver(kind);
        let capability = driver.reasoning_capability();
        assert_eq!(capability.maximum(), expected_maximum, "driver={kind:?}");
        assert_eq!(capability.mapping, expected_mapping, "driver={kind:?}");
        assert_eq!(driver.max_reasoning_level(), capability.maximum());
        assert_eq!(capability.resolve(ReasoningLevel::Off), ReasoningLevel::Off);
    }
}
