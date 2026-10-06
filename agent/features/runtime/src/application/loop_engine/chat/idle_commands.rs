//! idle 分支命令执行函数。
//!
//! 从旧 CommandRegistry 迁移，每个命令是独立函数。
//! 结果通过 RuntimeStreamEvent::CommandResultText { text, is_error } 回传 TUI。

use context::SessionManagementPort;
use memory::api::search::MemorySearchQuery;
use memory::api::{
    MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemoryPort, MemorySource, MemoryStats,
    WriteResult,
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
pub async fn execute_session(
    args: &str,
    session_id: &str,
    project: &share::session_types::ProjectIdentityData,
    session_management: &dyn SessionManagementPort,
) -> (String, bool) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return (format!("Current session: {}", session_id), false);
    }
    match parts[0] {
        "list" => {
            let sessions = match session_management.list_for_project(project).await {
                Ok(sessions) => sessions,
                Err(error) => return (format!("Failed to list sessions: {error}"), true),
            };
            let mut lines = String::from("📋 Sessions\n\n");
            for (i, s) in sessions.iter().take(15).enumerate() {
                lines.push_str(&format!(
                    "{}. {} ({} messages)\n",
                    i + 1,
                    s.id,
                    s.message_count
                ));
            }
            if sessions.is_empty() {
                lines.push_str("(no sessions)");
            }
            (lines, false)
        }
        // "new" 子命令已退役：`[action:new_session]` 文本协议从无 TUI 消费方，
        // 新会话由启动新进程承担；未知子命令落入下方兜底分支。
        "rename" => {
            if parts.len() < 3 {
                return ("Usage: /session rename <id> <name>".to_string(), true);
            }
            match session_management
                .update_metadata_for_project(
                    parts[1],
                    project,
                    context::SessionMetadataUpdateData {
                        title: Some(parts[2..].join(" ")),
                        ..Default::default()
                    },
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
            match session_management
                .delete_for_project(parts[1], project)
                .await
            {
                Ok(()) => (format!("Session {} deleted.", parts[1]), false),
                Err(error) => (format!("Failed to delete session: {error}"), true),
            }
        }
        "export" => {
            if parts.len() < 2 {
                return ("Usage: /session export <id>".to_string(), true);
            }
            match session_management
                .export_for_project(parts[1], project)
                .await
            {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(json) => (json, false),
                    Err(e) => (format!("Failed to encode session export: {e}"), true),
                },
                Err(e) => (format!("Failed to load session: {e}"), true),
            }
        }
        "import" => {
            if parts.len() < 2 {
                return ("Usage: /session import <file>".to_string(), true);
            }
            match tokio::fs::read(parts[1]).await {
                Ok(content) => match session_management
                    .import_for_project(&content, project)
                    .await
                {
                    Ok(session) => (format!("Session {} imported", session.id), false),
                    Err(e) => (format!("Failed to import session: {e}"), true),
                },
                Err(e) => (format!("Failed to read file: {e}"), true),
            }
        }
        _ => (format!("Unknown session command: {}", parts[0]), true),
    }
}

/// 执行 /memory 命令（非 remind 子命令）。
///
/// #871/#900：所有 memory 查询/变更均通过 `MemoryPort` API；调用方
///（loop_runner）负责通过 session-switch gate 捕获 `committed_memory` 后传入。
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
                let result = port.search(&query).await;
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
            let entries = candidates
                .iter()
                .map(|candidate| candidate.entry.clone())
                .collect::<Vec<_>>();
            let mut output = String::from("记忆数量已达上限，请先归档候选记忆：\n");
            output.push_str(&format_entry_list(&entries));
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
#[path = "idle_commands_tests.rs"]
mod tests;
