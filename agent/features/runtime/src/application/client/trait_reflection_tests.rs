use super::*;

#[test]
fn safe_sdk_view_contains_only_metadata_and_counts() {
    let view = summary_to_sdk(ReflectionSafeSummary {
        id: "reflection-1".into(),
        timestamp: 42,
        trigger: ReflectionTrigger::PreCompact,
        status: ReflectionStatus::Succeeded,
        deviations: 1,
        suggestions: 2,
        outdated: 3,
        apply_status: ReflectionApplyStatus::Applied,
        error_category: None,
        token_usage: Some(memory::api::reflection::ReflectionTokenUsage {
            input_tokens: 10,
            output_tokens: 20,
        }),
        duration_ms: 30,
    });
    assert_eq!(view.trigger, ReflectionTriggerView::PreCompact);
    assert_eq!(view.suggestions, 2);
    assert_eq!(view.token_usage.unwrap().input_tokens, 10);
}
