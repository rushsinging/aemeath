use super::*;

#[test]
fn agent_description_requires_self_contained_prompt_for_isolated_session() {
    let zh = agent("zh");
    assert!(zh.contains("全新的独立会话"));
    assert!(zh.contains("不继承"));
    assert!(zh.contains("prompt 必须自包含"));

    let en = agent("en");
    assert!(en.contains("fresh, independent session"));
    assert!(en.contains("inherits no context"));
    assert!(en.contains("prompt must be self-contained"));
}

/// 描述口径必须与 `AgentInput::data_schema()` 的 required 字段（`agent`）一致：
/// 出现即误导 LLM 漏传 `agent`（issue #1736 R1 复现）。
#[test]
fn agent_description_declares_agent_field_not_deprecated_role() {
    for lang in ["zh", "en"] {
        let description = agent(lang);
        assert!(
            !description.contains("config.agents.roles"),
            "{lang} 描述不得出现废弃口径 config.agents.roles：{description}"
        );
        assert!(
            !description.contains("`role`"),
            "{lang} 描述不得出现废弃口径 `role` 字段：{description}"
        );
        assert!(
            description.contains("`agent`"),
            "{lang} 描述必须声明 `agent` 字段口径：{description}"
        );
        assert!(
            description.contains("config.agents.names"),
            "{lang} 描述必须指向 config.agents.names：{description}"
        );
    }
}

#[test]
fn every_memory_tool_description_fits_the_200_char_budget() {
    for (zh, en) in [
        (super::memory_add("zh"), super::memory_add("en")),
        (super::memory_search("zh"), super::memory_search("en")),
        (super::memory_list("zh"), super::memory_list("en")),
        (super::memory_update("zh"), super::memory_update("en")),
        (super::memory_delete("zh"), super::memory_delete("en")),
    ] {
        assert!(zh.chars().count() <= 200, "zh too long: {zh}");
        assert!(en.chars().count() <= 200, "en too long: {en}");
    }
}

/// 本批收敛后的 description 长度预算：en/zh 均不得超过 200 字符。
#[test]
fn trimmed_core_descriptions_fit_the_200_char_budget() {
    for (zh, en) in [
        (agent("zh"), agent("en")),
        (ask_user("zh"), ask_user("en")),
        (memory_update("zh"), memory_update("en")),
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

#[test]
fn core_bilingual_and_fallback() {
    assert!(agent("zh").contains("启动一个新代理"));
    assert!(agent("en").contains("Launch a new agent"));
    assert_eq!(agent("fr"), agent("en"));
    assert!(memory_add("zh").contains("写入一条持久记忆"));
    assert!(skill("zh").contains("执行技能"));
    assert!(enter_plan_mode("zh").contains("进入计划模式"));
    assert!(exit_plan_mode("zh").contains("退出计划模式"));
    assert!(ask_user("zh").contains("向用户提问"));
    // options/questions 契约（对象格式、纯字符串被拒、Type something... 入口）
    // 已迁至 AskUserQuestionInput 字段 doc，由 tools crate 的 schema 测试锁定；
    // description 只保留 when-to-use。
    assert!(ask_user("zh").contains("需要用户输入或确认"));
    let ask_user_en = ask_user("en");
    assert!(ask_user_en.contains("wait for their response"));
    assert!(ask_user_en.contains("required"));
    assert!(brief("zh").contains("简要总结"));
    assert!(sleep("zh").contains("暂停执行"));
    assert!(tool_search("zh").contains("搜索可用工具"));
}
