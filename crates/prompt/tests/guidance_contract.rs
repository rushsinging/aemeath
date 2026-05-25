use prompt::api::guidance::{resolve_guidance, UNIVERSAL_EXECUTION_DISCIPLINE};
use std::collections::HashMap;

#[test]
fn test_prompt_guidance_resolves_config_fallback() {
    let mut guidance = HashMap::new();
    guidance.insert(
        "test-*".to_string(),
        "~/definitely-missing-guidance.md".to_string(),
    );

    let resolved = resolve_guidance("other-model", &guidance, false);

    assert!(!resolved.contains("definitely-missing-guidance"));
}

#[test]
fn test_prompt_guidance_exports_universal_execution_discipline() {
    assert!(UNIVERSAL_EXECUTION_DISCIPLINE.contains("Execution Discipline"));
}
