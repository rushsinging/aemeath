use runtime::application::reflection::ReflectionError;

#[test]
fn provider_failures_have_stable_display() {
    assert_eq!(
        ReflectionError::LlmCall.to_string(),
        "reflection LLM call failed"
    );
}
