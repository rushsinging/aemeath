//! idle 分支命令执行函数。
//!
//! 从旧 CommandRegistry 迁移，每个命令是独立函数。
//! 结果通过 RuntimeStreamEvent::CommandResultText { text, is_error } 回传 TUI。

/// 执行 /init 命令。force = true 时强制重新初始化。
pub fn execute_init(cwd: &str, force: bool) -> (String, bool) {
    use std::path::Path;
    // #2179：统一按「AGENTS.md 优先、CLAUDE.md fallback、都不存在新建
    // AGENTS.md」解析目标文件；非 force 时 NEVER 覆盖用户内容。
    let target = share::config::paths::project_instruction_write_target(Path::new(cwd));
    if target.exists() && !force {
        return (
            format!(
                "Already initialized ({} exists). Use /init force to re-initialize",
                target.display()
            ),
            true,
        );
    }
    let write_path = match resolve_symlink_chain(&target) {
        Ok(path) => path,
        Err(error) => {
            return (
                format!("Failed to resolve {}: {error}", target.display()),
                true,
            )
        }
    };
    let content = "# AGENTS.md\n\nProject instructions for aemeath.\n";
    if let Err(error) = write_atomically(&write_path, content) {
        return (
            format!("Failed to write {}: {error}", write_path.display()),
            true,
        );
    }
    (
        format!(
            "Project initialized successfully. Wrote {}.",
            write_path.display()
        ),
        false,
    )
}

/// 递归解析 symlink 链，返回应写入的最终普通文件路径。
///
/// 循环、深度超限或目标为目录时 fail-closed。
fn resolve_symlink_chain(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    const MAX_LINK_DEPTH: usize = 16;
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINK_DEPTH {
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error.to_string()),
        };
        if !metadata.file_type().is_symlink() {
            if metadata.is_dir() {
                return Err(format!("{} 是目录，不是指令文件", current.display()));
            }
            return Ok(current);
        }
        let link_target = std::fs::read_link(&current).map_err(|error| error.to_string())?;
        current = if link_target.is_absolute() {
            link_target
        } else {
            current
                .parent()
                .map(|parent| parent.join(&link_target))
                .unwrap_or(link_target)
        };
    }
    Err(format!(
        "symlink 深度超过 {MAX_LINK_DEPTH}：{}",
        path.display()
    ))
}

/// 同目录临时文件 + rename 的原子写入。
fn write_atomically(path: &std::path::Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension("aemeath-init.tmp");
    std::fs::write(&temp, content).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        error.to_string()
    })
}

#[cfg(test)]
mod init_tests {
    use super::*;

    #[test]
    fn init_creates_agents_md_when_neither_file_exists() {
        let root = tempfile::tempdir().expect("temp root");
        let (text, is_error) = execute_init(&root.path().to_string_lossy(), false);
        assert!(!is_error, "{text}");
        assert!(root.path().join("AGENTS.md").is_file());
        assert!(!root.path().join("CLAUDE.md").exists());
    }

    #[test]
    fn init_uses_existing_claude_md_instead_of_creating_agents_md() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join("CLAUDE.md"), "existing\n").expect("seed claude");
        let (text, is_error) = execute_init(&root.path().to_string_lossy(), true);
        assert!(!is_error, "{text}");
        assert!(!root.path().join("AGENTS.md").exists());
        let content = std::fs::read_to_string(root.path().join("CLAUDE.md")).expect("claude");
        assert!(content.contains("AGENTS.md"));
    }

    #[test]
    fn init_refuses_to_overwrite_without_force() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join("AGENTS.md"), "user content\n").expect("seed agents");
        let (_, is_error) = execute_init(&root.path().to_string_lossy(), false);
        assert!(is_error);
        assert_eq!(
            std::fs::read_to_string(root.path().join("AGENTS.md")).expect("agents"),
            "user content\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn init_writes_through_symlink_and_keeps_link() {
        let root = tempfile::tempdir().expect("temp root");
        let target = root.path().join("shared-instructions.md");
        std::fs::write(&target, "old\n").expect("seed target");
        std::os::unix::fs::symlink(&target, root.path().join("AGENTS.md")).expect("symlink");

        let (text, is_error) = execute_init(&root.path().to_string_lossy(), true);

        assert!(!is_error, "{text}");
        let link_meta =
            std::fs::symlink_metadata(root.path().join("AGENTS.md")).expect("link meta");
        assert!(link_meta.file_type().is_symlink());
        let content = std::fs::read_to_string(&target).expect("target content");
        assert!(content.contains("Project instructions for aemeath"));
    }
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
            // 覆盖式更新：读现值-改 title-回传整份 metadata。
            // 当前 active session：走 coordinator 同步 committed revision，避免 CAS 漂移。
            let result = if rename_id == session_id {
                let mut updated = coordinator.committed_session().metadata.clone();
                updated.title = Some(title.clone());
                coordinator
                    .update_active_session_metadata(updated)
                    .await
                    .map(|_| ())
            } else {
                match session_management
                    .load_for_project(rename_id, &project)
                    .await
                {
                    Ok(session) => {
                        let mut updated = session.metadata.clone();
                        updated.title = Some(title.clone());
                        session_management
                            .update_metadata_for_project(rename_id, &project, updated)
                            .await
                            .map(|_| ())
                    }
                    Err(error) => Err(error),
                }
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
