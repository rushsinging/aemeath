use super::{RuntimePresentation, RuntimePresentationIntent};
use crate::tui::view_model::status::ReasoningLevelView as ReasoningLevel;

#[test]
fn runtime_presentation_updates_provider_model_context_size_and_thinking() {
    let mut presentation = RuntimePresentation::default();

    presentation.apply(RuntimePresentationIntent::ProviderModel {
        provider: Some("anthropic".to_string()),
        model_id: Some("claude-opus".to_string()),
    });
    presentation.apply(RuntimePresentationIntent::ContextSize(200_000));
    presentation.apply(RuntimePresentationIntent::Thinking {
        enabled: false,
        level: ReasoningLevel::Off,
    });

    assert_eq!(presentation.provider(), Some("anthropic"));
    assert_eq!(presentation.model_id(), Some("claude-opus"));
    assert_eq!(presentation.context_size(), 200_000);
    assert!(!presentation.thinking());
    assert_eq!(presentation.reasoning_level(), ReasoningLevel::Off);
}

/// #1616：Thinking intent 必须同时携带深度，开关与 level 一起落到呈现状态。
#[test]
fn runtime_presentation_thinking_intent_updates_reasoning_level_too() {
    let mut presentation = RuntimePresentation::default();
    assert_eq!(presentation.reasoning_level(), ReasoningLevel::High);

    presentation.apply(RuntimePresentationIntent::Thinking {
        enabled: true,
        level: ReasoningLevel::Medium,
    });
    assert!(presentation.thinking());
    assert_eq!(presentation.reasoning_level(), ReasoningLevel::Medium);

    presentation.apply(RuntimePresentationIntent::Thinking {
        enabled: false,
        level: ReasoningLevel::Off,
    });
    assert!(!presentation.thinking());
    assert_eq!(presentation.reasoning_level(), ReasoningLevel::Off);
}
