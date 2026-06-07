use super::helpers::*;
use crate::api::ToolContext;
use share::memory_ops::{MemoryCategory, MemoryEntry, MemoryLayer, MemorySource};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

fn test_ctx(cwd: PathBuf) -> ToolContext {
    ToolContext {
        cwd: cwd.clone(),
        workspace: project::api::WorkspaceService::new(cwd.clone()),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        agent_runner: None,
        session_reminders: Some(Arc::new(Mutex::new(share::tool::SessionReminders::new()))),
        memory_config: share::config::MemoryConfig::default(),
        plan_mode: None,
        allow_all: false,
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: Some("test-session".to_string()),
    }
}

#[test]
fn test_validate_content_normal() {
    assert!(validate_content("记住这个决策").is_ok());
}

#[test]
fn test_validate_content_empty() {
    assert!(validate_content("   ").is_err());
}

#[test]
fn test_validate_content_too_long() {
    let content = "x".repeat(MAX_CONTENT_CHARS + 1);
    assert!(validate_content(&content).is_err());
}

#[test]
fn test_parse_tags_normal() {
    let input = serde_json::json!({"tags": ["rust", "rust", " memory "]});
    let tags = parse_tags(&input).unwrap();

    assert_eq!(tags, vec!["memory", "rust"]);
}

#[test]
fn test_parse_tags_empty_array() {
    let input = serde_json::json!({"tags": []});
    let tags = parse_tags(&input).unwrap();

    assert!(tags.is_empty());
}

#[test]
fn test_parse_tags_invalid_item() {
    let input = serde_json::json!({"tags": [1]});

    assert!(parse_tags(&input).is_err());
}

#[tokio::test]
async fn test_memory_tool_add_and_search() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let mut store = open_store_with_base(&ctx, dir.path().to_path_buf()).unwrap();
    let entry = MemoryEntry::new(
        "memory-1",
        100,
        MemoryLayer::Project,
        MemoryCategory::Decision,
        "使用 MemoryTool 管理记忆",
        MemorySource::Llm,
    );
    store.add(entry).unwrap();

    let results = store.search("MemoryTool", 5).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].content.contains("MemoryTool"));
}

#[test]
fn test_open_store_with_base_uses_memory_config_happy_path() {
    let dir = tempdir().unwrap();
    let mut ctx = test_ctx(dir.path().join("project"));
    ctx.memory_config.max_entries = 2;
    ctx.memory_config.similarity_threshold = 0.6;
    let store = open_store_with_base(&ctx, dir.path().to_path_buf()).unwrap();

    assert!(!store.needs_eviction(MemoryLayer::Project).unwrap());
}

#[test]
fn test_open_store_with_base_rejects_zero_max_entries() {
    let dir = tempdir().unwrap();
    let mut ctx = test_ctx(dir.path().join("project"));
    ctx.memory_config.max_entries = 0;

    let result = open_store_with_base(&ctx, dir.path().to_path_buf());

    assert!(matches!(
        result,
        Err(ref error) if error.contains("max_entries 必须大于 0")
    ));
}

#[test]
fn test_open_store_with_base_rejects_invalid_similarity_threshold() {
    let dir = tempdir().unwrap();
    let mut ctx = test_ctx(dir.path().join("project"));
    ctx.memory_config.similarity_threshold = 1.1;

    let result = open_store_with_base(&ctx, dir.path().to_path_buf());

    assert!(matches!(
        result,
        Err(ref error) if error.contains("similarity_threshold 必须在 0 到 1 之间")
    ));
}
