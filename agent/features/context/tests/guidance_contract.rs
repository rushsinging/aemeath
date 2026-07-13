use context::prompt::api::guidance::{resolve_guidance, universal_execution_discipline};
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
fn test_prompt_guidance_en_has_followup_classification() {
    let en = universal_execution_discipline("en");
    assert!(
        en.contains("handling_user_followups"),
        "EN guidance should have a dedicated followup classification block"
    );
    assert!(en.contains("INTERRUPT"));
    assert!(en.contains("NEW REQUEST"));
    assert!(en.contains("CLARIFICATION"));
    assert!(en.contains("ASIDE"));
    assert!(en.contains("INTERRUPT > NEW REQUEST > CLARIFICATION > ASIDE"));
}

#[test]
fn test_prompt_guidance_zh_has_followup_classification() {
    let zh = universal_execution_discipline("zh");
    assert!(
        zh.contains("handling_user_followups"),
        "ZH guidance should have a dedicated followup classification block"
    );
    assert!(zh.contains("INTERRUPT > NEW REQUEST > CLARIFICATION > ASIDE"));
    assert!(zh.contains("当用户在任务执行中发送新消息时"));
}

#[test]
fn test_prompt_guidance_en_mentions_task_list_updates() {
    let en = universal_execution_discipline("en");
    assert!(en.contains("When the user sends a new message"));
    assert!(en.contains("update the active task list"));
    assert!(en.contains("modify task descriptions, add tasks, remove tasks"));
}

#[test]
fn test_prompt_guidance_zh_mentions_task_list_updates() {
    let zh = universal_execution_discipline("zh");
    assert!(zh.contains("当用户在任务执行中发送新消息时"));
    assert!(zh.contains("更新活跃的 task list"));
    assert!(zh.contains("修改任务描述、添加任务、删除任务"));
}

#[test]
fn test_prompt_guidance_falls_back_to_en_for_unknown_lang() {
    let unknown = universal_execution_discipline("fr");
    let en = universal_execution_discipline("en");
    assert_eq!(unknown, en, "unknown language should fall back to English");
}

#[test]
fn test_prompt_guidance_exports_universal_execution_discipline() {
    let en = universal_execution_discipline("en");
    assert!(en.contains("Execution Discipline"));
    let zh = universal_execution_discipline("zh");
    assert!(zh.contains("执行纪律"));
}
