//! idle 分支命令执行函数。
//!
//! 从旧 CommandRegistry 迁移，每个命令是独立函数。
//! 结果通过 RuntimeStreamEvent::CommandResultText { text, is_error } 回传 TUI。

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
            let sessions = crate::business::session::list_sessions().await;
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
            match crate::business::session::update_session_metadata(
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
            match crate::business::session::load_session(parts[1]).await {
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
                Ok(content) => {
                    match serde_json::from_str::<crate::business::session::Session>(&content) {
                        Ok(session) => {
                            match crate::business::session::save_session(&session).await {
                                Ok(_) => (format!("Session {} imported", session.id), false),
                                Err(e) => (format!("Failed to save imported session: {}", e), true),
                            }
                        }
                        Err(e) => (format!("Failed to parse session file: {}", e), true),
                    }
                }
                Err(e) => (format!("Failed to read file: {}", e), true),
            }
        }
        _ => (format!("Unknown session command: {}", parts[0]), true),
    }
}

/// 执行 /memory 命令（非 remind 子命令）。
pub async fn execute_memory(args: &str, cwd: &str, mem: &MemoryConfig) -> (String, bool) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() || parts[0] == "list" {
        let store = match open_memory_store(cwd, mem).await {
            Ok(s) => s,
            Err(e) => return (format!("Failed to open memory store: {}", e), true),
        };
        match store.list(None) {
            Ok(entries) => {
                if entries.is_empty() {
                    return ("(no memories stored)".to_string(), false);
                }
                (share::memory::format_memory_list(&entries), false)
            }
            Err(e) => (format!("Failed to list memories: {}", e), true),
        }
    } else {
        match parts[0] {
            "add" => {
                if parts.len() < 2 {
                    return ("Usage: /memory add <content>".to_string(), true);
                }
                let content = parts[1..].join(" ");
                let mut store = match open_memory_store(cwd, mem).await {
                    Ok(s) => s,
                    Err(e) => return (format!("Failed to open memory store: {}", e), true),
                };
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let entry = share::memory::MemoryEntry::new(
                    uuid::Uuid::now_v7().to_string(),
                    now,
                    share::memory::MemoryLayer::Project,
                    share::memory::MemoryCategory::Fact,
                    content,
                    share::memory::MemorySource::User,
                );
                match store.add(entry) {
                    Ok(result) => (share::memory::format_add_result(result), false),
                    Err(e) => (format!("Failed to add memory: {}", e), true),
                }
            }
            "delete" | "del" | "remove" | "rm" => {
                if parts.len() < 2 {
                    return ("Usage: /memory delete <id>".to_string(), true);
                }
                let mut store = match open_memory_store(cwd, mem).await {
                    Ok(s) => s,
                    Err(e) => return (format!("Failed to open memory store: {}", e), true),
                };
                match store.delete(parts[1]) {
                    Ok(_) => (format!("Deleted memory: {}", parts[1]), false),
                    Err(e) => (format!("Failed: {}", e), true),
                }
            }
            "pin" | "unpin" => {
                if parts.len() < 2 {
                    return (format!("Usage: /memory {} <id>", parts[0]), true);
                }
                let mut store = match open_memory_store(cwd, mem).await {
                    Ok(s) => s,
                    Err(e) => return (format!("Failed to open memory store: {}", e), true),
                };
                let pin = parts[0] == "pin";
                match store.pin(parts[1], pin) {
                    Ok(_) => (
                        format!(
                            "Memory {} {}",
                            parts[1],
                            if pin { "pinned" } else { "unpinned" }
                        ),
                        false,
                    ),
                    Err(e) => (format!("Failed: {}", e), true),
                }
            }
            "search" => {
                if parts.len() < 2 {
                    return ("Usage: /memory search <query>".to_string(), true);
                }
                let query = parts[1..].join(" ");
                let store = match open_memory_store(cwd, mem).await {
                    Ok(s) => s,
                    Err(e) => return (format!("Failed to open memory store: {}", e), true),
                };
                match store.search(&query, 20) {
                    Ok(results) => {
                        if results.is_empty() {
                            return ("(no results)".to_string(), false);
                        }
                        (share::memory::format_memory_list(&results), false)
                    }
                    Err(e) => (format!("Failed: {}", e), true),
                }
            }
            "compact" => {
                let mut store = match open_memory_store(cwd, mem).await {
                    Ok(s) => s,
                    Err(e) => return (format!("Failed to open memory store: {}", e), true),
                };
                match store.compact() {
                    Ok(result) => (
                        format!(
                            "Memory compact 完成：归档 {} 条，剩余 {} 条。",
                            result.archived, result.remaining
                        ),
                        false,
                    ),
                    Err(e) => (format!("Failed: {}", e), true),
                }
            }
            "stats" => {
                let store = match open_memory_store(cwd, mem).await {
                    Ok(s) => s,
                    Err(e) => return (format!("Failed to open memory store: {}", e), true),
                };
                match store.stats(0) {
                    Ok(stats) => (
                        format!(
                            "📊 Memory Stats\n\n\
                             Global: {}\n\
                             Global archive: {}\n\
                             Project: {}\n\
                             Project archive: {}\n\
                             Reminders: {}",
                            stats.global_count,
                            stats.global_archive_count,
                            stats.project_count,
                            stats.project_archive_count,
                            stats.reminders_count,
                        ),
                        false,
                    ),
                    Err(e) => (format!("Failed: {}", e), true),
                }
            }
            _ => (format!("Unknown memory subcommand: {}", parts[0]), true),
        }
    }
}

/// 打开 memory store（从旧 memory_support.rs 提取）。
///
/// 接收已由调用方（composition 层）解析好的 `MemoryConfig`，避免 business 层
/// 反向依赖 core 的 ConfigAppService（COLA 分层：business 不得依赖 core）。
async fn open_memory_store(
    cwd: &str,
    mem: &MemoryConfig,
) -> Result<storage::api::MemoryStore, String> {
    use storage::api::{memory_base_dir, project_file_name, MemoryStore};

    if !mem.enabled {
        return Err("Memory 系统已禁用。".to_string());
    }
    MemoryStore::new(
        memory_base_dir(),
        project_file_name(cwd),
        mem.max_entries,
        mem.similarity_threshold,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::MemoryConfig;

    // ── open_memory_store ──────────────────────────────────────────────

    /// 回归 PR-C：enabled=true 时不应返回"已禁用"错误。
    ///
    /// 修复前 `open_memory_store` 内部用 `Config::default()`，
    /// 其 `memory.enabled` 恒为 false（注：此处为该函数旧实现的缺陷，
    /// 现已改为显式接收 `&MemoryConfig`）。本测试锁定注入路径生效。
    #[tokio::test]
    async fn test_open_memory_store_enabled_returns_store() {
        // Arrange
        let mem = MemoryConfig {
            enabled: true,
            max_entries: 100,
            similarity_threshold: 0.7,
            ..MemoryConfig::default()
        };

        // Act
        let result = open_memory_store("/tmp/aemeath-test-nonexistent", &mem).await;

        // Assert —— 成功构造 store；"已禁用"只会出现在 Err 分支，Ok 即证明未被禁用
        assert!(
            result.is_ok(),
            "enabled store should open, got err: {:?}",
            result.err()
        );
    }

    /// 回归 PR-C：enabled=false 时必须返回"已禁用"。
    ///
    /// 这正是修复前 `/memory` 命令永远报"已禁用"的根因所在——
    /// 现在只有显式禁用才会触发，本测试锁定该行为不被回退。
    #[tokio::test]
    async fn test_open_memory_store_disabled_returns_disabled_message() {
        // Arrange
        let mem = MemoryConfig {
            enabled: false,
            ..MemoryConfig::default()
        };

        // Act
        let result = open_memory_store("/tmp/aemeath-test-nonexistent", &mem).await;

        // Assert —— 用 match 避免 MemoryStore: Debug 约束
        match result {
            Ok(_) => panic!("disabled store must error, but got Ok"),
            Err(err) => assert!(
                err.contains("已禁用"),
                "disabled path should report disabled, got: {err}"
            ),
        }
    }

    // ── execute_memory ────────────────────────────────────────────────

    /// 回归 PR-C：通过 `execute_memory` 端到端验证禁用路径。
    ///
    /// `execute_memory` 会把 `open_memory_store` 的 `Err` 包裹为
    /// `"Failed to open memory store: ..."` 并标记 `is_error = true`。
    #[tokio::test]
    async fn test_execute_memory_disabled_returns_disabled_message() {
        // Arrange
        let mem = MemoryConfig {
            enabled: false,
            ..MemoryConfig::default()
        };

        // Act —— /memory 无参数（list 分支）
        let (text, is_error) = execute_memory("", "/tmp/aemeath-test-nonexistent", &mem).await;

        // Assert
        assert!(is_error, "disabled memory should be an error");
        assert!(
            text.contains("已禁用"),
            "should surface disabled message, got: {text}"
        );
    }

    /// 回归 PR-C：`execute_memory` 在 enabled=true 时绝不返回"已禁用"。
    ///
    /// 使用不存在的 cwd 触发 `list` 空结果路径，验证文本不含禁用字样。
    #[tokio::test]
    async fn test_execute_memory_enabled_does_not_return_disabled() {
        // Arrange
        let mem = MemoryConfig {
            enabled: true,
            max_entries: 100,
            similarity_threshold: 0.7,
            ..MemoryConfig::default()
        };

        // Act —— /memory 无参数（list 分支）
        let (text, is_error) = execute_memory("", "/tmp/aemeath-test-nonexistent", &mem).await;

        // Assert —— 非错误，且不含禁用字样
        assert!(
            !is_error,
            "enabled memory list should not be an error, got: {text}"
        );
        assert!(
            !text.contains("已禁用"),
            "enabled path must never surface disabled message, got: {text}"
        );
    }
}
