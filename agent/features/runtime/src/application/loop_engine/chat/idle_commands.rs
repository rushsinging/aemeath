//! idle 分支命令执行函数。
//!
//! 从旧 CommandRegistry 迁移，每个命令是独立函数。
//! 结果通过 RuntimeStreamEvent::CommandResultText { text, is_error } 回传 TUI。

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
    coordinator: &crate::SessionSwitchCoordinator,
    session_management: &dyn context::SessionManagementPort,
) -> (String, bool) {
    let project = coordinator.project_identity();
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return (format!("Current session: {}", session_id), false);
    }
    match parts[0] {
        "list" => {
            let sessions = match session_management.list_for_project(&project).await {
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
            let rename_id = parts[1];
            let title = parts[2..].join(" ");
            let update = context::SessionMetadataUpdateData {
                title: Some(title.clone()),
                ..Default::default()
            };
            // 当前 active session：走 wiring 同步 committed revision，避免 CAS 漂移。
            let result = if rename_id == session_id {
                coordinator
                    .update_active_session_metadata(update)
                    .await
                    .map(|_| ())
            } else {
                session_management
                    .update_metadata_for_project(rename_id, &project, update)
                    .await
                    .map(|_| ())
            };
            match result {
                Ok(()) => (format!("Session {} renamed to {}", rename_id, title), false),
                Err(e) => (format!("Failed to rename session: {}", e), true),
            }
        }
        "delete" => {
            if parts.len() < 2 {
                return ("Usage: /session delete <id>".to_string(), true);
            }
            match session_management
                .delete_for_project(parts[1], &project)
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
                .export_for_project(parts[1], &project)
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
                    .import_for_project(&content, &project)
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
