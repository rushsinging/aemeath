use super::{RuntimePresentation, RuntimePresentationIntent};

#[test]
fn runtime_presentation_updates_provider_model_context_size_and_thinking() {
    let mut presentation = RuntimePresentation::default();

    presentation.apply(RuntimePresentationIntent::ProviderModel {
        provider: Some("anthropic".to_string()),
        model_id: Some("claude-opus".to_string()),
    });
    presentation.apply(RuntimePresentationIntent::ContextSize(200_000));
    presentation.apply(RuntimePresentationIntent::Thinking(false));

    assert_eq!(presentation.provider(), Some("anthropic"));
    assert_eq!(presentation.model_id(), Some("claude-opus"));
    assert_eq!(presentation.context_size(), 200_000);
    assert!(!presentation.thinking());
}
