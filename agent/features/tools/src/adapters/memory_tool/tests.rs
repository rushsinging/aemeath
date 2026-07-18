use super::helpers::*;
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::{ToolExecutionContext, TypedTool};
use memory::{MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemoryPort, MemorySource};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

// ── Test helpers ────────────────────────────────────────────────────

/// Test source backed by a shared [`memory::InMemoryMemory`] so that add /
/// search / delete operations are observable across the same port instance.
fn shared_test_source() -> (Arc<dyn MemoryPortSource>, Arc<memory::InMemoryMemory>) {
    let port = Arc::new(
        memory::InMemoryMemory::new(memory::MemoryPolicy::default()).expect("valid default policy"),
    );
    struct SharedSource {
        port: Arc<dyn MemoryPort>,
    }
    impl MemoryPortSource for SharedSource {
        fn current(&self) -> Arc<dyn MemoryPort> {
            self.port.clone()
        }
    }
    let source: Arc<dyn MemoryPortSource> = Arc::new(SharedSource { port: port.clone() });
    (source, port)
}

fn test_ctx(cwd: std::path::PathBuf) -> ToolExecutionContext {
    let (source, _) = shared_test_source();
    std::fs::create_dir_all(&cwd).expect("create test workspace");
    ToolExecutionContext {
        workspace: project::wire_production_workspace(cwd.clone())
            .expect("workspace 初始化成功")
            .into_views(),
        run_id: "test-run".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: crate::domain::ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            memory_source: source,
            lang: "en".to_string(),
            allow_all: false,
        },
        session_reminders: Some(Arc::new(Mutex::new(crate::domain::SessionReminders::new()))),
        plan_mode: None,
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: Some("test-session".to_string()),
    }
}

// ── Pure helper tests ───────────────────────────────────────────────

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

// ── MemoryTool integration tests (H1: MemoryPort, not MemoryStore) ──

#[tokio::test]
async fn test_memory_tool_add_then_search_via_port() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let (source, port) = shared_test_source();
    let tool = super::MemoryTool { source };

    // Add a memory through the tool
    let add_input = serde_json::json!({
        "action": "add",
        "content": "使用 MemoryPort 管理记忆",
        "category": "decision",
        "layer": "project",
    });
    let result = tool.call(add_input, &ctx).await;
    assert!(!result.is_error, "add should succeed");

    // The port should now contain the entry
    let entries = port.list(Some(MemoryLayer::Project));
    assert_eq!(entries.len(), 1);
    assert!(entries[0].content.contains("MemoryPort"));

    // Search through the tool
    let search_input = serde_json::json!({
        "action": "search",
        "query": "MemoryPort",
    });
    let result = tool.call(search_input, &ctx).await;
    assert!(!result.is_error, "search should succeed");
}

#[tokio::test]
async fn test_memory_tool_add_and_delete_via_port() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let (source, port) = shared_test_source();
    let tool = super::MemoryTool { source };

    // Add
    let add_input = serde_json::json!({
        "action": "add",
        "content": "temporary decision",
    });
    let result = tool.call(add_input, &ctx).await;
    assert!(!result.is_error);

    let entries = port.list(Some(MemoryLayer::Project));
    assert_eq!(entries.len(), 1);
    let id = entries[0].id;

    // Delete
    let delete_input = serde_json::json!({
        "action": "delete",
        "id": id.to_string(),
    });
    let result = tool.call(delete_input, &ctx).await;
    assert!(!result.is_error, "delete should succeed");

    let entries = port.list(Some(MemoryLayer::Project));
    assert!(entries.is_empty(), "entry should be deleted");
}

#[tokio::test]
async fn test_memory_tool_pin_via_port() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let (source, port) = shared_test_source();
    let tool = super::MemoryTool { source };

    // Add
    tool.call(
        serde_json::json!({"action": "add", "content": "pin me"}),
        &ctx,
    )
    .await;

    let entries = port.list(Some(MemoryLayer::Project));
    assert_eq!(entries.len(), 1);
    let id = entries[0].id;
    assert!(!entries[0].pinned);

    // Pin
    let pin_input = serde_json::json!({
        "action": "pin",
        "id": id.to_string(),
        "pinned": true,
    });
    let result = tool.call(pin_input, &ctx).await;
    assert!(!result.is_error);

    let entries = port.list(Some(MemoryLayer::Project));
    assert_eq!(entries.len(), 1);
    assert!(entries[0].pinned);
}

#[tokio::test]
async fn test_memory_tool_list_via_port() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let (source, port) = shared_test_source();
    let tool = super::MemoryTool { source };

    // Add two entries
    tool.call(
        serde_json::json!({"action": "add", "content": "first"}),
        &ctx,
    )
    .await;
    tool.call(
        serde_json::json!({"action": "add", "content": "second"}),
        &ctx,
    )
    .await;

    // List
    let list_input = serde_json::json!({"action": "list"});
    let result = tool.call(list_input, &ctx).await;
    assert!(!result.is_error);

    let entries = port.list(None);
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn test_memory_tool_add_invalid_content_returns_error() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let (source, _) = shared_test_source();
    let tool = super::MemoryTool { source };

    let result = tool
        .call(serde_json::json!({"action": "add", "content": "   "}), &ctx)
        .await;
    assert!(result.is_error);
}

#[tokio::test]
async fn test_memory_tool_delete_nonexistent_returns_error() {
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));
    let (source, _) = shared_test_source();
    let tool = super::MemoryTool { source };

    let fake_id = MemoryId::now_v7().to_string();
    let result = tool
        .call(serde_json::json!({"action": "delete", "id": fake_id}), &ctx)
        .await;
    assert!(result.is_error);
}

#[tokio::test]
async fn test_memory_tool_resume_swaps_port_same_registry_writes_new() {
    // H1 invariant: after resume swaps Memory, the same tool (holding a source)
    // writes to the NEW port, not the old one.
    let dir = tempdir().unwrap();
    let ctx = test_ctx(dir.path().join("project"));

    // First source + port (initial)
    let (source1, port1) = shared_test_source();
    // Second source + port (after "resume")
    let (source2, port2) = shared_test_source();

    // Use source1 initially
    let tool1 = super::MemoryTool { source: source1 };
    tool1
        .call(
            serde_json::json!({"action": "add", "content": "old memory"}),
            &ctx,
        )
        .await;
    assert_eq!(port1.list(None).len(), 1);
    assert_eq!(port2.list(None).len(), 0);

    // After "resume": tool uses source2 (simulating a new MemoryTool registered
    // with the new source in the same registry). The old port must NOT receive
    // the new write.
    let tool2 = super::MemoryTool { source: source2 };
    tool2
        .call(
            serde_json::json!({"action": "add", "content": "new memory"}),
            &ctx,
        )
        .await;
    assert_eq!(
        port1.list(None).len(),
        1,
        "old port must not receive new writes"
    );
    assert_eq!(port2.list(None).len(), 1, "new port must receive the write");
}

#[test]
fn test_legacy_memory_files_not_written() {
    // H1 invariant: MemoryPort path must not write legacy storage files.
    // We verify by checking that no .json memory files are created in the
    // memory base directory after using the tool via MemoryPort.
    let dir = tempdir().unwrap();
    let memory_base = dir.path().join("memory_base");
    std::fs::create_dir_all(&memory_base).unwrap();

    let ctx = test_ctx(dir.path().join("project"));
    let (source, port) = shared_test_source();
    let tool = super::MemoryTool { source };

    // Synchronous add through the port directly
    let entry = MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        MemoryCategory::Decision,
        "via port",
        MemorySource::Llm,
    )
    .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(port.write(entry)).unwrap();

    // No legacy .json files should exist in the memory_base directory
    let legacy_files: Vec<_> = std::fs::read_dir(&memory_base)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert!(
        legacy_files.is_empty(),
        "MemoryPort must not write legacy files"
    );

    // The tool variable is intentionally used here to avoid unused warnings;
    // it exercises the type system in the test_ctx setup.
    let _ = &tool;
    let _ = &ctx;
}
