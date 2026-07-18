//! idle 分支命令执行函数。
//!
//! 从旧 CommandRegistry 迁移，每个命令是独立函数。
//! 结果通过 RuntimeStreamEvent::CommandResultText { text, is_error } 回传 TUI。

use memory::{
    MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemoryPort, MemorySearchQuery,
    MemorySource, MemoryStats, WriteResult,
};
use share::config::MemoryConfig;

/// 执行 /init 命令。force = true 时强制重新初始化。
pub fn execute_init(cwd: &str, force: bool) -> (String, bool) {
    use std::path::Path;
    let claude_md = Path::new(cwd).join("CLAUDE.md");
    let agents_dir = Path::new(cwd).join(".aemeath");
    if claude_md.exists() && !force {
        return (
            "Already initialized. Use /init force to re-initialize".to_string(),
            true,
        );
    }
    // 创建 .aemeath 目录
    if let Err(e) = std::fs::create_dir_all(&agents_dir) {
        return (format!("Failed to create .aemeath directory: {}", e), true);
    }
    // 写入 CLAUDE.md（如果不存在或 force）
    if !claude_md.exists() || force {
        let content = "# AGENTS.md\n\nProject instructions for aemeath.\n";
        if let Err(e) = std::fs::write(&claude_md, content) {
            return (format!("Failed to write CLAUDE.md: {}", e), true);
        }
    }
    (
        "Project initialized successfully. Created .aemeath/ directory and CLAUDE.md.".to_string(),
        false,
    )
}

/// 执行 /session 命令。
pub async fn execute_session(args: &str, session_id: &str) -> (String, bool) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return (format!("Current session: {}", session_id), false);
    }
    match parts[0] {
        "list" => {
            let sessions = context::session::list_sessions().await;
            let mut lines = String::from("📋 Sessions\n\n");
            for (i, s) in sessions.iter().take(15).enumerate() {
                lines.push_str(&format!(
                    "{}. {} ({} messages)\n",
                    i + 1,
                    s.id,
                    s.messages.len()
                ));
            }
            if sessions.is_empty() {
                lines.push_str("(no sessions)");
            }
            (lines, false)
        }
        "new" => {
            // NewSession action — 通知 TUI 创建新会话
            // 通过返回特殊文本来触发 TUI 行为
            ("[action:new_session]".to_string(), false)
        }
        "rename" => {
            if parts.len() < 3 {
                return ("Usage: /session rename <id> <name>".to_string(), true);
            }
            match context::session::update_session_metadata(
                parts[1],
                Some(parts[2].to_string()),
                None,
                None,
                None,
            )
            .await
            {
                Ok(_) => (
                    format!("Session {} renamed to {}", parts[1], parts[2]),
                    false,
                ),
                Err(e) => (format!("Failed to rename session: {}", e), true),
            }
        }
        "delete" => {
            if parts.len() < 2 {
                return ("Usage: /session delete <id>".to_string(), true);
            }
            // 返回确认提示（事件流不直接执行删除，需要 TUI 二次确认）
            (format!("[confirm:delete_session:{}]", parts[1]), false)
        }
        "export" => {
            if parts.len() < 2 {
                return ("Usage: /session export <id>".to_string(), true);
            }
            match context::session::load_session(parts[1]).await {
                Ok(session) => match serde_json::to_string_pretty(&session) {
                    Ok(json) => (json, false),
                    Err(e) => (format!("Failed to serialize session: {}", e), true),
                },
                Err(e) => (format!("Failed to load session: {}", e), true),
            }
        }
        "import" => {
            if parts.len() < 2 {
                return ("Usage: /session import <file>".to_string(), true);
            }
            match tokio::fs::read_to_string(parts[1]).await {
                Ok(content) => match serde_json::from_str::<context::session::Session>(&content) {
                    Ok(session) => match context::session::save_session(&session).await {
                        Ok(_) => (format!("Session {} imported", session.id), false),
                        Err(e) => (format!("Failed to save imported session: {}", e), true),
                    },
                    Err(e) => (format!("Failed to parse session file: {}", e), true),
                },
                Err(e) => (format!("Failed to read file: {}", e), true),
            }
        }
        _ => (format!("Unknown session command: {}", parts[0]), true),
    }
}

/// 执行 /memory 命令（非 remind 子命令）。
///
/// #871：所有 memory 查询/变更均通过 `MemoryPort` API，不再直接打开
/// `storage::MemoryStore`。调用方（loop_runner）负责通过 session-switch gate
/// 捕获 `committed_memory` 后传入。
pub async fn execute_memory(
    args: &str,
    port: &dyn MemoryPort,
    config: &MemoryConfig,
) -> (String, bool) {
    if !config.enabled {
        return ("Memory 系统已禁用。".to_string(), true);
    }

    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() || parts[0] == "list" {
        let entries = port.list(None);
        if entries.is_empty() {
            return ("(no memories stored)".to_string(), false);
        }
        (format_entry_list(&entries), false)
    } else {
        match parts[0] {
            "add" => {
                if parts.len() < 2 {
                    return ("Usage: /memory add <content>".to_string(), true);
                }
                let content = parts[1..].join(" ");
                let now = unix_now();
                let entry = match MemoryEntry::new(
                    MemoryId::now_v7(),
                    now,
                    MemoryLayer::Project,
                    MemoryCategory::Fact,
                    content,
                    MemorySource::User,
                ) {
                    Ok(entry) => entry,
                    Err(e) => return (format!("Failed to create memory entry: {e}"), true),
                };
                match port.write(entry).await {
                    Ok(result) => (format_write_result(&result), false),
                    Err(e) => (format!("Failed to add memory: {e}"), true),
                }
            }
            "delete" | "del" | "remove" | "rm" => {
                if parts.len() < 2 {
                    return ("Usage: /memory delete <id>".to_string(), true);
                }
                let id = match MemoryId::new(parts[1]) {
                    Ok(id) => id,
                    Err(e) => return (format!("Invalid memory id: {e}"), true),
                };
                match port.delete(&id).await {
                    Ok(true) => (format!("Deleted memory: {id}"), false),
                    Ok(false) => (format!("Memory not found: {id}"), true),
                    Err(e) => (format!("Failed: {e}"), true),
                }
            }
            "pin" | "unpin" => {
                if parts.len() < 2 {
                    return (format!("Usage: /memory {} <id>", parts[0]), true);
                }
                let id = match MemoryId::new(parts[1]) {
                    Ok(id) => id,
                    Err(e) => return (format!("Invalid memory id: {e}"), true),
                };
                let pin = parts[0] == "pin";
                match port.pin(&id, pin).await {
                    Ok(true) => (
                        format!("Memory {id} {}", if pin { "pinned" } else { "unpinned" }),
                        false,
                    ),
                    Ok(false) => (format!("Memory not found: {id}"), true),
                    Err(e) => (format!("Failed: {e}"), true),
                }
            }
            "search" => {
                if parts.len() < 2 {
                    return ("Usage: /memory search <query>".to_string(), true);
                }
                let text = parts[1..].join(" ");
                let query = MemorySearchQuery {
                    text,
                    limit: 20,
                    layer: None,
                    category: None,
                    include_archive: false,
                    now: unix_now(),
                };
                let result = port.search(&query);
                if result.hits.is_empty() {
                    return ("(no results)".to_string(), false);
                }
                let entries: Vec<MemoryEntry> =
                    result.hits.iter().map(|hit| hit.entry.clone()).collect();
                (format_entry_list(&entries), false)
            }
            "compact" => match port.compact().await {
                Ok(result) => (
                    format!(
                        "Memory compact 完成：归档 {} 条，剩余 {} 条。",
                        result.archived, result.remaining
                    ),
                    false,
                ),
                Err(e) => (format!("Failed: {e}"), true),
            },
            "stats" => (format_stats(&port.stats()), false),
            _ => (format!("Unknown memory subcommand: {}", parts[0]), true),
        }
    }
}

// ── formatting helpers (share::memory DTO → memory crate types) ──────────

fn format_entry_list(entries: &[MemoryEntry]) -> String {
    if entries.is_empty() {
        return "暂无记忆。".to_string();
    }
    let mut output = String::new();
    for entry in entries {
        output.push_str(&format_single_entry(entry));
    }
    output
}

fn format_single_entry(entry: &MemoryEntry) -> String {
    let status = if entry.pinned { "pinned" } else { "active" };
    let tags = if entry.tags.is_empty() {
        String::new()
    } else {
        format!(" #{}", entry.tags.join(" #"))
    };
    format!(
        "- {} [{} {:?}/{:?}] {}{}\n",
        entry.id, status, entry.layer, entry.category, entry.content, tags
    )
}

fn format_write_result(result: &WriteResult) -> String {
    match result {
        WriteResult::Added { id } => {
            format!("记忆已添加。ID: {id}")
        }
        WriteResult::Merged { existing_id } => {
            format!("已与相似记忆合并: {existing_id}")
        }
        WriteResult::NeedsEviction { candidates } => {
            let mut output = String::from("记忆数量已达上限，请先归档候选记忆：\n");
            output.push_str(&format_entry_list(candidates));
            output
        }
        WriteResult::NoOp => "记忆未变更。".to_string(),
    }
}

fn format_stats(stats: &MemoryStats) -> String {
    format!(
        "📊 Memory Stats\n\n\
         Global: {}\n\
         Global archive: {}\n\
         Project: {}\n\
         Project archive: {}",
        stats.global_count,
        stats.global_archive_count,
        stats.project_count,
        stats.project_archive_count,
    )
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory::{InMemoryMemory, MemoryPolicy};

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
}
