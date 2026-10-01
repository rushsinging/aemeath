use super::*;

#[test]
fn static_system_prompt_bilingual_and_fallback_en() {
    let zh = static_system_prompt("zh");
    let en = static_system_prompt("en");
    assert!(zh.contains("交互式软件工程 agent"));
    assert!(en.contains("interactive software-engineering agent"));
    assert_eq!(static_system_prompt("fr"), en);
}

#[test]
fn static_system_prompt_contains_placeholders() {
    for s in [static_system_prompt("zh"), static_system_prompt("en")] {
        assert!(s.contains("{cwd_str}"));
        assert!(s.contains("{is_git}"));
        assert!(s.contains("path_base"));
        assert!(s.contains("workspace_root"));
    }
}

#[test]
fn static_system_prompt_requires_self_contained_agent_prompts() {
    let zh = static_system_prompt("zh");
    assert!(zh.contains("子代理是隔离会话"));
    assert!(zh.contains("每个 prompt 必须自包含"));
    assert!(zh.contains("目标、背景、精确范围、约束、验证方式和期望输出"));

    let en = static_system_prompt("en");
    assert!(en.contains("Sub-agents are isolated sessions"));
    assert!(en.contains("self-contained prompt"));
    assert!(en
        .contains("goal, background, exact scope, constraints, verification, and expected output"));
}

#[test]
fn static_system_prompt_keeps_parallel_safe_contract_concise() {
    for s in [static_system_prompt("zh"), static_system_prompt("en")] {
        assert!(s.contains("parallel-safe"));
    }
}

#[test]
fn static_system_prompt_locks_memory_priority_and_superseded_visibility() {
    let en = static_system_prompt("en");
    assert!(en
        .contains("Memory must never override system, safety, or the current user's instructions"));
    assert!(en.contains("A superseded memory (non-empty superseded_by) is no longer injected"));
    assert!(en.contains("still surfaced by MemorySearch and MemoryList"));

    let zh = static_system_prompt("zh");
    assert!(zh.contains("绝不能覆盖系统、安全与当前用户指令"));
    assert!(zh.contains("superseded_by 非空）不再自动注入"));
    assert!(zh.contains("MemorySearch 与 MemoryList 查到"));
}

#[test]
fn static_system_prompt_locks_worktree_command_discipline() {
    let en = static_system_prompt("en");
    assert!(en.contains("Use EnterWorktree to work on another branch"));
    assert!(en.contains("NEVER use `git checkout -b`"));
    assert!(en.contains("NEVER use it to switch to an arbitrary directory"));

    let zh = static_system_prompt("zh");
    assert!(zh.contains("需要在其他分支上工作时用 EnterWorktree"));
    assert!(zh.contains("NEVER 在主 checkout 里用 `git checkout -b`"));
    assert!(zh.contains("NEVER 用它切换到任意目录"));
}

#[test]
fn static_system_prompt_core_contract_item_count_is_fourteen() {
    for s in [static_system_prompt("zh"), static_system_prompt("en")] {
        assert_eq!(s.matches("\n- ").count(), 14);
    }
}
