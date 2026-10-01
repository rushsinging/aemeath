use super::*;
use memory::api::{InMemoryMemory, MemoryPolicy};

/// Creates a fresh `InMemoryMemory` port for each test (no filesystem IO).
fn test_port() -> InMemoryMemory {
    InMemoryMemory::new(MemoryPolicy {
        max_entries: 100,
        similarity_threshold: 0.9,
    })
    .expect("valid policy")
}

fn enabled_config() -> MemoryConfig {
    MemoryConfig {
        enabled: true,
        ..MemoryConfig::default()
    }
}

fn disabled_config() -> MemoryConfig {
    MemoryConfig {
        enabled: false,
        ..MemoryConfig::default()
    }
}

// ── disabled path ─────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_disabled_returns_disabled_message() {
    let port = test_port();
    let (text, is_error) = execute_memory("", &port, &disabled_config()).await;
    assert!(is_error, "disabled memory should be an error");
    assert!(
        text.contains("已禁用"),
        "should surface disabled message, got: {text}"
    );
}

#[tokio::test]
async fn test_execute_memory_enabled_does_not_return_disabled() {
    let port = test_port();
    let (text, is_error) = execute_memory("", &port, &enabled_config()).await;
    assert!(
        !is_error,
        "enabled memory list should not be an error, got: {text}"
    );
    assert!(
        !text.contains("已禁用"),
        "enabled path must never surface disabled message, got: {text}"
    );
}

// ── list ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_list_empty() {
    let port = test_port();
    let (text, is_error) = execute_memory("", &port, &enabled_config()).await;
    assert!(!is_error);
    assert_eq!(text, "(no memories stored)");
}

#[tokio::test]
async fn test_execute_memory_list_after_add() {
    let port = test_port();
    execute_memory("add hello world", &port, &enabled_config()).await;

    let (text, is_error) = execute_memory("", &port, &enabled_config()).await;
    assert!(!is_error);
    assert!(
        text.contains("hello world"),
        "list should show entry: {text}"
    );
}

// ── add ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_add_success() {
    let port = test_port();
    let (text, is_error) = execute_memory("add my fact", &port, &enabled_config()).await;
    assert!(!is_error, "add should succeed: {text}");
    assert!(text.contains("记忆已添加"), "got: {text}");
    // The full UUID is included so the user can reference it with delete/pin.
    assert!(
        text.contains("ID: 0"),
        "add result should include a UUID v7 (starts with 0): {text}"
    );
}

#[tokio::test]
async fn test_execute_memory_add_missing_content() {
    let port = test_port();
    let (text, is_error) = execute_memory("add", &port, &enabled_config()).await;
    assert!(is_error);
    assert!(text.contains("Usage"));
}

// ── delete ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_delete_invalid_uuid_returns_clear_error() {
    let port = test_port();
    let (text, is_error) = execute_memory("delete not-a-uuid", &port, &enabled_config()).await;
    assert!(is_error);
    assert!(
        text.contains("Invalid memory id"),
        "should surface clear UUID parse error, got: {text}"
    );
}

#[tokio::test]
async fn test_execute_memory_delete_valid_uuid_not_found() {
    let port = test_port();
    let (text, is_error) = execute_memory(
        "delete 01890f3c-7c00-7000-8000-000000000001",
        &port,
        &enabled_config(),
    )
    .await;
    assert!(is_error);
    assert!(
        text.contains("not found"),
        "non-existent id should report not found, got: {text}"
    );
}

#[tokio::test]
async fn test_execute_memory_add_then_delete_roundtrip() {
    let port = test_port();
    // add
    let (add_text, _) = execute_memory("add roundtrip fact", &port, &enabled_config()).await;
    // extract full UUID from "记忆已添加。ID: <uuid>"
    let id = add_text.rsplit("ID: ").next().unwrap().trim();
    assert!(
        MemoryId::new(id).is_ok(),
        "add result should include valid UUID: {id}"
    );

    // delete using the extracted UUID
    let (del_text, del_error) =
        execute_memory(&format!("delete {id}"), &port, &enabled_config()).await;
    assert!(!del_error, "delete should succeed: {del_text}");
    assert!(del_text.contains("Deleted"));

    // verify list is empty again
    let (list_text, _) = execute_memory("", &port, &enabled_config()).await;
    assert_eq!(list_text, "(no memories stored)");
}

// ── pin / unpin ────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_pin_invalid_uuid_returns_clear_error() {
    let port = test_port();
    let (text, is_error) = execute_memory("pin nope", &port, &enabled_config()).await;
    assert!(is_error);
    assert!(
        text.contains("Invalid memory id"),
        "should surface clear UUID parse error, got: {text}"
    );
}

#[tokio::test]
async fn test_execute_memory_add_then_pin_then_unpin() {
    let port = test_port();
    let (add_text, _) = execute_memory("add pinnable", &port, &enabled_config()).await;
    let id = add_text.rsplit("ID: ").next().unwrap().trim();

    // pin
    let (pin_text, pin_error) =
        execute_memory(&format!("pin {id}"), &port, &enabled_config()).await;
    assert!(!pin_error, "pin should succeed: {pin_text}");
    assert!(pin_text.contains("pinned"));

    // verify pinned shows in list
    let (list_text, _) = execute_memory("", &port, &enabled_config()).await;
    assert!(
        list_text.contains("pinned"),
        "list should show pinned: {list_text}"
    );

    // unpin
    let (unpin_text, unpin_error) =
        execute_memory(&format!("unpin {id}"), &port, &enabled_config()).await;
    assert!(!unpin_error, "unpin should succeed: {unpin_text}");
    assert!(unpin_text.contains("unpinned"));
}

// ── search ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_search_no_results() {
    let port = test_port();
    let (text, is_error) = execute_memory("search nothing", &port, &enabled_config()).await;
    assert!(!is_error);
    assert_eq!(text, "(no results)");
}

#[tokio::test]
async fn test_execute_memory_search_finds_entry() {
    let port = test_port();
    execute_memory("add rust memory port", &port, &enabled_config()).await;

    let (text, is_error) = execute_memory("search rust", &port, &enabled_config()).await;
    assert!(!is_error);
    assert!(
        text.contains("rust memory port"),
        "search should find matching entry: {text}"
    );
}

// ── compact ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_compact_empty() {
    let port = test_port();
    let (text, is_error) = execute_memory("compact", &port, &enabled_config()).await;
    assert!(!is_error);
    assert!(
        text.contains("compact") || text.contains("归档"),
        "compact result: {text}"
    );
}

// ── stats ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_stats() {
    let port = test_port();
    let (text, is_error) = execute_memory("stats", &port, &enabled_config()).await;
    assert!(!is_error);
    assert!(text.contains("Memory Stats"), "got: {text}");
    assert!(
        text.contains("Global: 0"),
        "stats should show zero counts: {text}"
    );

    // add one and re-check
    execute_memory("add stat test", &port, &enabled_config()).await;
    let (text, _) = execute_memory("stats", &port, &enabled_config()).await;
    assert!(
        text.contains("Project: 1"),
        "stats should reflect added entry: {text}"
    );
}

// ── unknown subcommand ─────────────────────────────────────────────

#[tokio::test]
async fn test_execute_memory_unknown_subcommand() {
    let port = test_port();
    let (text, is_error) = execute_memory("bogus arg", &port, &enabled_config()).await;
    assert!(is_error);
    assert!(text.contains("Unknown memory subcommand"));
}
