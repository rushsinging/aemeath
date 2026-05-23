use super::*;
use aemeath_core::memory::{MemoryCategory, MemoryEntry, MemoryLayer, MemorySource};

#[test]
fn test_static_prompt_requires_task_update_for_direct_tools() {
    let text = static_system_prompt_for_test("/tmp/project", true);

    assert!(text.contains("BEFORE starting work on a task yourself"));
    assert!(text.contains("Read/Grep/Glob/Bash/Edit/Write"));
    assert!(text.contains("TaskUpdate(taskId, status=\"in_progress\")"));
    assert!(text.contains("AFTER completing a task yourself"));
    assert!(text.contains("TaskListCreate before TaskCreate"));
    assert!(text.contains("TaskListComplete"));
}

#[test]
fn test_static_prompt_delegates_agent_task_status_to_task_id() {
    let text = static_system_prompt_for_test("/tmp/project", true);

    assert!(text.contains("pass `taskId` to the Agent tool"));
    assert!(text.contains("do NOT call TaskUpdate for that task"));
    assert!(!text.contains("TaskUpdate(id2, in_progress) → Agent"));
}

#[test]
fn test_static_prompt_says_task_reminders_may_be_unrelated() {
    let text = static_system_prompt_for_test("/tmp/project", true);

    assert!(text.contains("When the user says \"continue\""));
    assert!(text.contains("call TaskList first"));
    assert!(text.contains("may refer to older task batches"));
    assert!(text.contains("prioritize the latest user request"));
}

#[test]
fn test_build_commit_guidance_includes_provider_model_trailer() {
    let guidance = build_commit_guidance(Some("zhipu"), Some("glm-5.1"));

    assert!(guidance.contains("# Commit Message Guidance"));
    assert!(guidance.contains("git log --format=%B --grep='Co-Authored-By'"));
    assert!(guidance.contains(
        "Co-Authored-By: Aemeath (zhipu/glm-5.1) <github:rushsinging/aemeath>"
    ));
    assert!(guidance.contains("Do not invent human co-authors"));
}

#[test]
fn test_build_commit_guidance_uses_unknown_fallback() {
    let guidance = build_commit_guidance(None, None);

    assert!(guidance.contains(
        "Co-Authored-By: Aemeath (unknown/unknown) <github:rushsinging/aemeath>"
    ));
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

    let previous = std::env::var_os("AEMEATH_AGENTS_DIR");
    std::env::set_var("AEMEATH_AGENTS_DIR", &agents_dir);

    let hook_runner = HookRunner::new(Default::default(), base.to_string_lossy().to_string());
    let content = load_agents_md(&base, &hook_runner).await;

    if let Some(previous) = previous {
        std::env::set_var("AEMEATH_AGENTS_DIR", previous);
    } else {
        std::env::remove_var("AEMEATH_AGENTS_DIR");
    }

    assert!(content.contains("old project instructions"));
    assert!(!base.join("AGENTS.md").exists());

    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn test_format_memory_context_empty() {
    assert!(format_memory_context(&[]).is_none());
}

#[test]
fn test_format_memory_context_with_entries() {
    let entry = MemoryEntry::new(
        MemoryLayer::Project,
        MemoryCategory::Decision,
        "使用 JSON 存储 Memory",
        MemorySource::User,
    );
    let output = format_memory_context(&[entry]).unwrap();

    assert!(output.contains("# Project Memory"));
    assert!(output.contains("[Decision]"));
    assert!(output.contains("使用 JSON 存储 Memory"));
}

#[tokio::test]
async fn test_collect_memory_context_zero_limit() {
    let cwd = PathBuf::from("/tmp/aemeath-no-memory");

    assert!(collect_memory_context_with_limit(&cwd, 0).await.is_none());
}

#[test]
fn test_memory_context_options_from_config_happy_path() {
    let config = aemeath_core::config::MemoryConfig {
        enabled: true,
        max_entries: 42,
        max_inject_count: 3,
        auto_summary_on_session_end: true,
        similarity_threshold: 0.7,
        reflection: Default::default(),
    };
    let options = memory_context_options_from_config(&config);

    assert_eq!(options.max_entries, 42);
    assert_eq!(options.max_inject_count, 3);
    assert_eq!(options.similarity_threshold, 0.7);
}

#[test]
fn test_memory_context_options_from_config_boundary_zero_inject_count() {
    let config = aemeath_core::config::MemoryConfig {
        enabled: true,
        max_entries: 42,
        max_inject_count: 0,
        auto_summary_on_session_end: true,
        similarity_threshold: 0.7,
        reflection: Default::default(),
    };
    let options = memory_context_options_from_config(&config);

    assert_eq!(options.max_entries, 42);
    assert_eq!(options.max_inject_count, 0);
    assert_eq!(options.similarity_threshold, 0.7);
}

#[test]
fn test_memory_context_options_from_config_disabled_uses_zero_inject_count() {
    let config = aemeath_core::config::MemoryConfig {
        enabled: false,
        max_entries: 42,
        max_inject_count: 3,
        auto_summary_on_session_end: true,
        similarity_threshold: 0.7,
        reflection: Default::default(),
    };
    let options = memory_context_options_from_config(&config);

    assert_eq!(options.max_entries, 42);
    assert_eq!(options.max_inject_count, 0);
    assert_eq!(options.similarity_threshold, 0.7);
}
