use super::*;
use crate::utils::bootstrap::config_paths::TestEnvGuard;

#[test]
fn test_static_prompt_requires_task_update_for_direct_tools() {
    let text = static_system_prompt_for_test("/tmp/project", true, "en");

    assert!(text.contains("BEFORE starting work on a task yourself"));
    assert!(text.contains("Read/Grep/Glob/Bash/Edit/Write"));
    assert!(text.contains("TaskUpdate(taskId, status=\"in_progress\")"));
    assert!(text.contains("AFTER completing a task yourself"));
    assert!(text.contains("TaskListCreate before TaskCreate"));
    assert!(text.contains("TaskListComplete"));
}

#[test]
fn test_static_prompt_delegates_agent_task_status_to_task_id() {
    let text = static_system_prompt_for_test("/tmp/project", true, "en");

    assert!(text.contains("pass `taskId` to the Agent tool"));
    assert!(text.contains("taskId is NOT required"));
    assert!(!text.contains("TaskUpdate(id2, in_progress) → Agent"));
}

#[test]
fn test_static_prompt_says_task_reminders_may_be_unrelated() {
    let text = static_system_prompt_for_test("/tmp/project", true, "en");

    assert!(text.contains("When the user says \"continue\""));
    assert!(text.contains("call TaskList first"));
    assert!(text.contains("may refer to older task batches"));
    assert!(text.contains("prioritize the latest user request"));
}

#[test]
fn test_static_prompt_mentions_memory_tool_without_memory_contents() {
    let text = static_system_prompt_for_test("/tmp/project", true, "en");

    assert!(
        text.contains("Use the Memory tool to search and manage long-term memory when relevant")
    );
    assert!(text.contains("Do not assume memory contents unless retrieved"));
    assert!(!text.contains("# Project Memory"));
}

#[test]
fn test_static_prompt_guides_worktree_relative_paths_without_fixed_workspace_root() {
    let text = static_system_prompt_for_test(
        "/tmp/project/.worktrees/fix-bug-69-worktree-cwd",
        true,
        "en",
    );

    assert!(!text.contains("Current workspace root"));
    assert!(text.contains("Prefer relative paths"));
    assert!(text.contains("Do not reuse absolute paths from another checkout"));
}

#[test]
fn test_build_commit_guidance_includes_provider_model_trailer() {
    let guidance = build_commit_guidance(Some("zhipu"), Some("glm-5.1"), "en");

    assert!(guidance.contains("# Commit Message Guidance"));
    assert!(guidance.contains("Before creating any git commit, invoke the built-in `commit` skill"));
    assert!(guidance.contains("git log --format=%B --grep='Co-Authored-By'"));
    assert!(
        guidance.contains("Co-Authored-By: Aemeath (zhipu/glm-5.1) <github:rushsinging/aemeath>")
    );
    assert!(guidance.contains("Do not invent human co-authors"));
}

#[test]
fn test_build_commit_guidance_uses_unknown_fallback() {
    let guidance = build_commit_guidance(None, None, "en");

    assert!(
        guidance.contains("Co-Authored-By: Aemeath (unknown/unknown) <github:rushsinging/aemeath>")
    );
}

#[test]
fn test_prompt_context_new_preserves_model_metadata() {
    let cwd = PathBuf::from("/tmp/example");
    let context = PromptContext::new(&cwd, Some("openrouter"), Some("anthropic/claude-sonnet-4"));

    assert_eq!(context.cwd, cwd);
    assert_eq!(context.provider_name.as_deref(), Some("openrouter"));
    assert_eq!(
        context.model_name.as_deref(),
        Some("anthropic/claude-sonnet-4")
    );
}

#[test]
fn test_project_instruction_walk_includes_cwd_first() {
    let tmp = std::env::temp_dir().join(format!(
        "aemeath_test_walk_cwd_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let paths = project_instruction_walk(&tmp, 2);

    assert_eq!(paths[0], tmp.join("CLAUDE.md"));
    assert_eq!(paths[1], tmp.join("AGENTS.md"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_project_instruction_walk_includes_parent() {
    let tmp = std::env::temp_dir().join(format!(
        "aemeath_test_walk_parent_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let child = tmp.join("sub");
    std::fs::create_dir_all(&child).unwrap();

    let paths = project_instruction_walk(&child, 1);

    assert!(paths.contains(&child.join("CLAUDE.md")));
    assert!(paths.contains(&tmp.join("CLAUDE.md")));
    let child_idx = paths
        .iter()
        .position(|p| p == &child.join("CLAUDE.md"))
        .unwrap();
    let parent_idx = paths
        .iter()
        .position(|p| p == &tmp.join("CLAUDE.md"))
        .unwrap();
    assert!(child_idx < parent_idx);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_project_instruction_walk_depth_zero_cwd_only() {
    let tmp = std::env::temp_dir().join(format!(
        "aemeath_test_walk_zero_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let paths = project_instruction_walk(&tmp, 0);

    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], tmp.join("CLAUDE.md"));
    assert_eq!(paths[1], tmp.join("AGENTS.md"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_project_instruction_walk_does_not_scan_descendants_when_climbing_parents() {
    let tmp = std::env::temp_dir().join(format!(
        "aemeath_test_walk_parent_descendants_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let child = tmp.join("child");
    let sibling_nested = tmp.join("sibling").join("nested");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&sibling_nested).unwrap();

    let paths = project_instruction_walk(&child, 1);

    assert!(paths.contains(&child.join("CLAUDE.md")));
    assert!(paths.contains(&tmp.join("CLAUDE.md")));
    assert!(!paths.iter().any(|p| p.starts_with(tmp.join("sibling"))));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn test_build_system_prompt_parts_includes_commit_guidance() {
    let cwd = std::env::temp_dir().join(format!(
        "aemeath_commit_guidance_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&cwd).unwrap();
    let hook_runner = HookRunner::empty(cwd.display().to_string());
    let memory_config = MemoryConfig::default();
    let context = PromptContext::new(&cwd, Some("deepseek"), Some("deepseek-chat"));

    let parts = build_system_prompt_parts(&context, &hook_runner, &memory_config, "en").await;

    std::fs::remove_dir_all(cwd).unwrap();

    assert!(parts.dynamic_part.contains("# Commit Message Guidance"));
    assert!(parts
        .dynamic_part
        .contains("Before creating any git commit, invoke the built-in `commit` skill"));
    assert!(parts
        .dynamic_part
        .contains("Co-Authored-By: Aemeath (deepseek/deepseek-chat) <github:rushsinging/aemeath>"));
    assert!(!parts.dynamic_part.contains("Commit Style Context:"));
}

#[tokio::test]
async fn test_load_agents_md_prefers_project_claude_md() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_agents_md_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("AGENTS.md"), "project agents instructions").unwrap();
    std::fs::write(base.join("CLAUDE.md"), "old project instructions").unwrap();

    let hook_runner = HookRunner::new(Default::default(), base.to_string_lossy().to_string());
    let content = load_agents_md(&base, &hook_runner).await;

    assert!(content.contains("old project instructions"));
    assert!(!content.contains("project agents instructions"));

    std::fs::remove_dir_all(base).unwrap();
}

#[tokio::test]
async fn test_load_agents_md_falls_back_to_project_agents_md() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_agents_md_fallback_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("AGENTS.md"), "project agents instructions").unwrap();

    let hook_runner = HookRunner::new(Default::default(), base.to_string_lossy().to_string());
    let content = load_agents_md(&base, &hook_runner).await;

    assert!(content.contains("project agents instructions"));

    std::fs::remove_dir_all(base).unwrap();
}

#[tokio::test]
async fn test_load_agents_md_reads_project_claude_md_without_migration() {
    let base = std::env::temp_dir().join(format!(
        "aemeath_agents_md_no_auto_migration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("CLAUDE.md"), "old project instructions").unwrap();
    let agents_dir = base.join("agents-home");
    std::fs::create_dir_all(&agents_dir).unwrap();

    let _guard = TestEnvGuard::set("AEMEATH_AGENTS_DIR", &agents_dir);

    let hook_runner = HookRunner::new(Default::default(), base.to_string_lossy().to_string());
    let content = load_agents_md(&base, &hook_runner).await;

    assert!(content.contains("old project instructions"));
    assert!(!base.join("AGENTS.md").exists());

    std::fs::remove_dir_all(base).unwrap();
}
