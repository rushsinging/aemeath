//! idle 分支命令执行函数。
//!
//! 从旧 CommandRegistry 迁移，每个命令是独立函数。
//! 结果通过 RuntimeStreamEvent::CommandResultText { text, is_error } 回传 TUI。

use share::config::Config;

/// 执行 /cost 命令。args: "" / "session" = 当前会话, "total" = 全部。
pub async fn execute_cost(args: &str, session_id: &str) -> (String, bool) {
    use crate::business::cost::CostTracker;
    let mut tracker = CostTracker::new();
    let _ = tracker.load();
    match args.trim().to_lowercase().as_str() {
        "" | "session" => (tracker.session_summary(session_id).format(), false),
        "total" => (tracker.summary().format(), false),
        _ => (format!("Unknown argument: {}", args.trim()), true),
    }
}

/// 执行 /status 命令。
pub fn execute_status(
    config: &Config,
    session_id: &str,
    cwd: &str,
    current_model: &str,
) -> (String, bool) {
    use share::config::PermissionModeConfig;
    let permission_emoji = match config.permissions.mode {
        PermissionModeConfig::Ask => "🔔",
        PermissionModeConfig::AutoRead => "📖",
        PermissionModeConfig::AllowAll => "🔓",
    };
    let permission_text = match config.permissions.mode {
        PermissionModeConfig::Ask => "ask",
        PermissionModeConfig::AutoRead => "auto-read",
        PermissionModeConfig::AllowAll => "allow-all",
    };
    let markdown_icon = if config.ui.markdown { "✅" } else { "❌" };
    let tui_icon = if config.ui.tui { "✅" } else { "❌" };
    let base_url = config
        .api
        .base_url
        .as_deref()
        .unwrap_or("https://api.anthropic.com");
    let info = format!(
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         📊 Session Status\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         🆔 Session ID\n│ {}\n\
         📁 Working directory\n│ {}\n\
         🤖 Model\n│ {}\n\
         📏 Max tokens\n│ {}\n\
         🔐 Permission mode {}\n│ {}\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         ⚙️ Configuration\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         🌐 Base URL\n│ {}\n\
         📝 Markdown {}\n│ {}\n\
         🖥️  TUI {}\n│ {}\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        session_id,
        cwd,
        current_model,
        config.model.max_tokens,
        permission_emoji,
        permission_text,
        base_url,
        markdown_icon,
        if config.ui.markdown {
            "enabled"
        } else {
            "disabled"
        },
        tui_icon,
        if config.ui.tui { "enabled" } else { "disabled" },
    );
    (info, false)
}

/// 执行 /config 命令。args: "" = 查看, "get <key>" = 获取值, "reset" = 确认重置。
pub fn execute_config(args: &str, config: &Config) -> (String, bool) {
    use share::config::PermissionModeConfig;
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        let output = format!(
            "Current Configuration:\n\n\
             API:\n  Model: {}\n  Max tokens: {}\n  Base URL: {}\n\n\
             UI:\n  Markdown: {}\n  Color: {}\n  TUI: {}\n\n\
             Permissions:\n  Mode: {}\n\n\
             Storage:\n  Persist sessions: {}\n",
            config.model.name,
            config.model.max_tokens,
            config
                .api
                .base_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com"),
            config.ui.markdown,
            config.ui.color,
            config.ui.tui,
            match config.permissions.mode {
                PermissionModeConfig::Ask => "ask",
                PermissionModeConfig::AutoRead => "auto-read",
                PermissionModeConfig::AllowAll => "allow-all",
            },
            config.storage.persist_sessions,
        );
        (output, false)
    } else {
        match parts[0] {
            "get" => {
                if parts.len() < 2 {
                    return ("Usage: /config get <key>".to_string(), true);
                }
                let val = match parts[1] {
                    "model" => config.model.name.clone(),
                    "max_tokens" => config.model.max_tokens.to_string(),
                    "base_url" => config
                        .api
                        .base_url
                        .clone()
                        .unwrap_or_else(|| "default".to_string()),
                    "context_size" => config.model.context_size.to_string(),
                    "permission_mode" => match config.permissions.mode {
                        PermissionModeConfig::Ask => "ask".to_string(),
                        PermissionModeConfig::AutoRead => "auto-read".to_string(),
                        PermissionModeConfig::AllowAll => "allow-all".to_string(),
                    },
                    _ => "unknown key".to_string(),
                };
                (format!("{} = {}", parts[1], val), false)
            }
            "set" => (
                "`/config set` is not yet implemented. Edit ~/.agents/aemeath.json directly."
                    .to_string(),
                true,
            ),
            "reset" => (
                "Configuration reset requires confirmation. \
                 This feature is not yet available via event stream."
                    .to_string(),
                true,
            ),
            _ => (format!("Unknown config command: {}", parts[0]), true),
        }
    }
}

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

/// 执行 /stats 命令。
pub async fn execute_stats(args: &str, _session_id: &str, config: &Config) -> (String, bool) {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" | "all" => {
            let sessions = crate::business::session::list_sessions().await;
            let session_count = sessions.len();
            let info = format!(
                "📊 Statistics Overview\n\n\
                 Sessions: {}\n\
                 Model: {}\n\
                 Max tokens: {}\n\n\
                 Use /stats session for session details\n\
                 Use /stats tokens for token estimation",
                session_count, config.model.name, config.model.max_tokens,
            );
            (info, false)
        }
        "session" | "sessions" => {
            let sessions = crate::business::session::list_sessions().await;
            let mut lines = String::from("📋 Session History\n\n");
            for (i, s) in sessions.iter().take(10).enumerate() {
                lines.push_str(&format!("{}. {} ({})\n", i + 1, s.id, s.messages.len()));
            }
            if sessions.is_empty() {
                lines.push_str("(no sessions)");
            }
            (lines, false)
        }
        "tokens" => (
            "Use /context for current token usage estimation.".to_string(),
            false,
        ),
        "tools" => (
            "Tool usage statistics not yet available.".to_string(),
            false,
        ),
        _ => (format!("Unknown stats type: {}", arg), true),
    }
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
pub async fn execute_memory(args: &str, cwd: &str) -> (String, bool) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() || parts[0] == "list" {
        let store = match open_memory_store(cwd) {
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
                let mut store = match open_memory_store(cwd) {
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
                let mut store = match open_memory_store(cwd) {
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
                let mut store = match open_memory_store(cwd) {
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
                let store = match open_memory_store(cwd) {
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
                let mut store = match open_memory_store(cwd) {
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
                let store = match open_memory_store(cwd) {
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
fn open_memory_store(cwd: &str) -> Result<storage::api::MemoryStore, String> {
    use storage::api::{memory_base_dir, project_file_name, MemoryStore};

    let config = share::config::Config::default();
    if !config.memory.enabled {
        return Err("Memory 系统已禁用。".to_string());
    }
    MemoryStore::new(
        memory_base_dir(),
        project_file_name(cwd),
        config.memory.max_entries,
        config.memory.similarity_threshold,
    )
    .map_err(|e| e.to_string())
}
