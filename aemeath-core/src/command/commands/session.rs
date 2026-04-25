use crate::command::{Command, CommandAction, CommandCategory, CommandContext, CommandResult, ConfirmAction};
use crate::session;
use std::future::Future;
use std::pin::Pin;

/// Resume command - resume a previous session
pub fn resume_command() -> Command {
    Command::new_async(
        "resume".to_string(),
        "Resume a previous session".to_string(),
        CommandCategory::Session,
        resume_execute,
    )
    .with_usage(vec![
        "/resume - List recent sessions".to_string(),
        "/resume <id> - Resume specific session".to_string(),
    ])
    .with_aliases(vec!["r".to_string()])
}

fn resume_execute(args: String, _ctx: &mut CommandContext) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> {
    Box::pin(async move {
        let arg = args.trim();
        if arg.is_empty() {
            let sessions = session::list_sessions().await;
            if sessions.is_empty() {
                return CommandResult::Success("No saved sessions found".to_string());
            }
            let mut output = String::from("Recent Sessions:\n\n");
            for (i, sess) in sessions.iter().take(10).enumerate() {
                output.push_str(&format!(
                    "{}. {} - {} msgs - {} - {}\n",
                    i + 1,
                    sess.id,
                    sess.messages.len(),
                    sess.summary(),
                    sess.updated_at
                ));
            }
            output.push_str("\nUse /resume <id> to resume a session\n");
            CommandResult::Success(output)
        } else {
            let session_id = if arg.chars().all(|c| c.is_ascii_digit()) {
                let sessions = session::list_sessions().await;
                let idx: usize = arg.parse().unwrap_or(0);
                if idx == 0 || idx > sessions.len() {
                    return CommandResult::Error(format!("Invalid session index: {}", idx));
                }
                sessions[idx - 1].id.clone()
            } else {
                arg.to_string()
            };
            CommandResult::Action(CommandAction::ResumeSession(session_id))
        }
    })
}

/// Session command - manage sessions
pub fn session_command() -> Command {
    Command::new_async(
        "session".to_string(),
        "Manage sessions".to_string(),
        CommandCategory::Session,
        session_execute,
    )
    .with_usage(vec![
        "/session - List all sessions".to_string(),
        "/session info - Show current session info".to_string(),
        "/session new - Start new session".to_string(),
        "/session list - List all sessions".to_string(),
        "/session delete <id> - Delete a session".to_string(),
        "/session title <title> - Set session title".to_string(),
        "/session tag <tag> - Add a tag".to_string(),
        "/session untag <tag> - Remove a tag".to_string(),
        "/session favorite - Mark as favorite".to_string(),
        "/session unfavorite - Remove favorite".to_string(),
        "/session notes <notes> - Add notes".to_string(),
        "/session search <query> - Search sessions".to_string(),
    ])
    .with_aliases(vec!["sessions".to_string()])
}

fn session_execute(args: String, ctx: &mut CommandContext) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> {
    let session_id = ctx.session_id.clone();
    Box::pin(async move {
        let parts: Vec<&str> = args.trim().split_whitespace().collect();
            if parts.is_empty() || parts[0] == "list" {
                let sessions = session::list_sessions().await;
                if sessions.is_empty() {
                    return CommandResult::Success("No saved sessions found".to_string());
                }
                let mut output = format!("Saved Sessions ({}):\n\n", sessions.len());
                for sess in sessions.iter().take(20) {
                    let favorite_marker = if sess.metadata.is_favorite { "★ " } else { "  " };
                    output.push_str(&format!(
                        "{}{} {}\n  Messages: {} | Project: {} | Updated: {}\n",
                        favorite_marker,
                        sess.id,
                        sess.summary(),
                        sess.messages.len(),
                        sess.metadata.project.as_deref().unwrap_or("unknown"),
                        sess.updated_at
                    ));
                    if !sess.metadata.tags.is_empty() {
                        output.push_str(&format!("  Tags: {}\n", sess.metadata.tags.join(", ")));
                    }
                    output.push_str("\n");
                }
                if sessions.len() > 20 {
                    output.push_str(&format!("... and {} more sessions\n", sessions.len() - 20));
                }
                output.push_str("\nUse /resume <id> to resume a session\n");
                CommandResult::Success(output)
            } else {
                match parts[0] {
                    "info" => {
                        let sessions = session::list_sessions().await;
                        let current = sessions.iter().find(|s| s.id == session_id);
                        let mut output = format!(
                            "Current Session:\n  ID: {}\n  Messages: {}\n",
                            session_id,
                            current.map(|s| s.messages.len()).unwrap_or(0)
                        );
                        if let Some(sess) = current {
                            if let Some(title) = &sess.metadata.title {
                                output.push_str(&format!("  Title: {}\n", title));
                            }
                            if !sess.metadata.tags.is_empty() {
                                output.push_str(&format!("  Tags: {}\n", sess.metadata.tags.join(", ")));
                            }
                            if sess.metadata.is_favorite {
                                output.push_str("  Favorite: yes\n");
                            }
                            if let Some(notes) = &sess.metadata.notes {
                                output.push_str(&format!("  Notes: {}\n", notes));
                            }
                        }
                        output.push_str(&format!("\nSaved sessions: {}", sessions.len()));
                        CommandResult::Success(output)
                    }
                    "new" => CommandResult::Action(CommandAction::NewSession),
                    "delete" => {
                    if parts.len() < 2 {
                        return CommandResult::Error("Usage: /session delete <id>".to_string());
                    }
                    CommandResult::Confirm {
                        message: format!("Delete session {}?", parts[1]),
                        action: ConfirmAction::DeleteSession(parts[1].to_string()),
                    }
                }
                "save" => {
                    CommandResult::Success(format!("Session {} saved", session_id))
                }
                "title" => {
                    if parts.len() < 2 {
                        return CommandResult::Error("Usage: /session title <title>".to_string());
                    }
                    let title = parts[1..].join(" ");
                    match session::update_session_metadata(&session_id, Some(title), None, None, None).await {
                        Ok(sess) => CommandResult::Success(format!("Session title set to: {}", sess.metadata.title.unwrap_or_default())),
                        Err(e) => CommandResult::Error(e),
                    }
                }
                "tag" => {
                    if parts.len() < 2 {
                        return CommandResult::Error("Usage: /session tag <tag>".to_string());
                    }
                    let tag = parts[1];
                    match session::load_session(&session_id).await {
                        Ok(mut sess) => {
                            sess.add_tag(tag.to_string());
                            if let Err(e) = session::save_session(&sess).await {
                                return CommandResult::Error(e);
                            }
                            CommandResult::Success(format!("Tag '{}' added to session", tag))
                        }
                        Err(e) => CommandResult::Error(e),
                    }
                }
                "untag" => {
                    if parts.len() < 2 {
                        return CommandResult::Error("Usage: /session untag <tag>".to_string());
                    }
                    let tag = parts[1];
                    match session::load_session(&session_id).await {
                        Ok(mut sess) => {
                            sess.remove_tag(tag);
                            if let Err(e) = session::save_session(&sess).await {
                                return CommandResult::Error(e);
                            }
                            CommandResult::Success(format!("Tag '{}' removed from session", tag))
                        }
                        Err(e) => CommandResult::Error(e),
                    }
                }
                "favorite" => {
                    match session::update_session_metadata(&session_id, None, None, None, Some(true)).await {
                        Ok(_) => CommandResult::Success("Session marked as favorite".to_string()),
                        Err(e) => CommandResult::Error(e),
                    }
                }
                "unfavorite" => {
                    match session::update_session_metadata(&session_id, None, None, None, Some(false)).await {
                        Ok(_) => CommandResult::Success("Session removed from favorites".to_string()),
                        Err(e) => CommandResult::Error(e),
                    }
                }
                "notes" => {
                    if parts.len() < 2 {
                        return CommandResult::Error("Usage: /session notes <notes>".to_string());
                    }
                    let notes = parts[1..].join(" ");
                    match session::update_session_metadata(&session_id, None, None, Some(notes), None).await {
                        Ok(sess) => CommandResult::Success(format!("Notes updated: {}", sess.metadata.notes.unwrap_or_default())),
                        Err(e) => CommandResult::Error(e),
                    }
                }
                "search" => {
                    if parts.len() < 2 {
                        return CommandResult::Error("Usage: /session search <query>".to_string());
                    }
                    let query = parts[1..].join(" ");
                    let filter = session::SessionFilter {
                        title: Some(query.clone()),
                        project: Some(query.clone()),
                        ..Default::default()
                    };
                    let results = session::search_sessions(&filter).await;
                    if results.is_empty() {
                        CommandResult::Success("No matching sessions found".to_string())
                    } else {
                        let mut output = format!("Found {} sessions:\n\n", results.len());
                        for sess in results.iter().take(10) {
                            output.push_str(&format!("{} {}\n  {}\n\n", sess.id, sess.display_title(), sess.updated_at));
                        }
                        CommandResult::Success(output)
                    }
                }
                _ => CommandResult::Error(format!("Unknown session command: {}", parts[0])),
            }
        }
    })
}

/// Rewind command - rewind history
pub fn rewind_command() -> Command {
    Command::new(
        "rewind".to_string(),
        "Rewind message history".to_string(),
        CommandCategory::Session,
        rewind_execute,
    )
    .with_usage(vec![
        "/rewind - Show rewind options".to_string(),
        "/rewind <n> - Rewind n messages".to_string(),
        "/rewind to <id> - Rewind to specific message".to_string(),
    ])
}

fn rewind_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();
    if parts.is_empty() {
        CommandResult::Success(
            "Rewind Options:\n\nUse /rewind to remove messages from the session history.\n\nUsage:\n  /rewind <n> - Remove last n messages\n  /rewind to <id> - Rewind to message ID\n\nNote: Use session management for full history control".to_string()
        )
    } else {
        match parts[0] {
            "to" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /rewind to <id>".to_string());
                }
                CommandResult::Success(format!("Rewind to message: {}", parts[1]))
            }
            n => {
                let count: usize = n.parse().unwrap_or(1);
                CommandResult::Success(format!("Rewind {} messages", count))
            }
        }
    }
}
