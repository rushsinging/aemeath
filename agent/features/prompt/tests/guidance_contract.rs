use prompt::api::guidance::{resolve_guidance, UNIVERSAL_EXECUTION_DISCIPLINE};
use std::collections::HashMap;

#[test]
fn test_prompt_guidance_resolves_config_fallback() {
    let mut guidance = HashMap::new();
    guidance.insert(
        "test-*".to_string(),
        "~/definitely-missing-guidance.md".to_string(),
    );

    let resolved = resolve_guidance("other-model", &guidance, false, "en");

    assert!(!resolved.contains("definitely-missing-guidance"));
}

#[test]
fn test_prompt_guidance_mentions_task_list_updates_when_user_changes_scope() {
    assert!(
        UNIVERSAL_EXECUTION_DISCIPLINE.contains("When the user asks a question"),
        "guidance should explicitly cover user questions during active task execution"
    );
    assert!(
        UNIVERSAL_EXECUTION_DISCIPLINE.contains("update the active task list"),
        "guidance should require updating task lists when the request scope changes"
    );
    assert!(
        UNIVERSAL_EXECUTION_DISCIPLINE
            .contains("modify task descriptions, add tasks, remove tasks"),
        "guidance should mention modifying descriptions and adding/removing tasks"
    );
}

#[test]
fn test_prompt_guidance_exports_universal_execution_discipline() {
    assert!(UNIVERSAL_EXECUTION_DISCIPLINE.contains("Execution Discipline"));
}
