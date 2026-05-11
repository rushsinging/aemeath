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
