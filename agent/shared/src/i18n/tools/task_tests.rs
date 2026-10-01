use super::*;

#[test]
fn task_bilingual_and_fallback() {
    assert!(task_create("zh").contains("仅为复杂的多步骤工作"));
    assert!(task_create("en").contains("Create a task to track progress"));
    assert_eq!(task_create("fr"), task_create("en"));
    assert!(task_get("zh").contains("按 ID 检索任务"));
    assert!(task_list("zh").contains("列出所有任务"));
    assert!(task_lists("zh").contains("历史任务列表"));
    assert!(task_stop("zh").contains("停止"));
    assert!(task_block_by("zh").contains("完整替换"));
    assert!(task_block_by("en").contains("Replace all"));
    // 校验会拒绝的前提（ID 归属 + 无环）schema 未承载，必须留在 description。
    assert!(task_block_by("zh").contains("当前任务列表"));
    assert!(task_block_by("zh").contains("不得形成环"));
    assert!(task_block_by("en").contains("current task list"));
    assert!(task_block_by("en").contains("acyclic"));
    // when-to-use：保留「仅当…时使用」正向前提。
    assert!(task_list("zh").contains("仅当"));
    assert!(task_list("en").contains("Use only when"));
    // 合法 key 列表属参数契约，走 schema 字段注释（由 tools crate 的
    // task_update_schema_only_advertises_supported_fields 锁定）；
    // description 只保留 when-to-use，这里锁定其不含被移除的 key 枚举。
    for (text, phrase) in [
        (task_update("zh"), "单个字段"),
        (task_update("en"), "single field"),
    ] {
        assert!(text.contains(phrase));
        assert!(!text.contains("blocked_by_id"));
        assert!(!text.contains("owner:"));
    }
    assert!(task_list_create("zh").contains("创建任务列表"));
    assert!(task_list_complete("zh").contains("完成当前活动任务列表"));
}

/// 收敛后的 description 长度预算：en/zh 均不得超过 200 字符。
#[test]
fn trimmed_task_descriptions_fit_the_200_char_budget() {
    for (zh, en) in [
        (task_block_by("zh"), task_block_by("en")),
        (task_list("zh"), task_list("en")),
    ] {
        assert!(
            zh.chars().count() <= 200,
            "zh too long ({}): {zh}",
            zh.chars().count()
        );
        assert!(
            en.chars().count() <= 200,
            "en too long ({}): {en}",
            en.chars().count()
        );
    }
}
