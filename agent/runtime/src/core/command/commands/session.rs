//! Session management commands: resume, session, rewind.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{
    Command, CommandAction, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
    ConfirmAction,
};
use std::future::Future;
use std::pin::Pin;

inventory::submit! {
    CommandDescriptor::new(|| {
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
    })
}

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new_async(
            "session".to_string(),
            "Manage conversation sessions".to_string(),
            CommandCategory::Session,
            session_execute,
        )
        .with_usage(vec![
            "/session - Show current session".to_string(),
            "/session list - List all sessions".to_string(),
            "/session new - Start a new session".to_string(),
            "/session rename <id> <name> - Rename a session".to_string(),
            "/session delete <id> - Delete a session".to_string(),
            "/session export <id> - Export a session to JSON".to_string(),
            "/session import <file> - Import a session from JSON".to_string(),
        ])
        .with_aliases(vec!["sessions".to_string()])
    })
}

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "rewind".to_string(),
            "Rewind conversation to a previous point".to_string(),
            CommandCategory::Session,
            rewind_execute,
        )
        .with_usage(vec![
            "/rewind - Show message numbers".to_string(),
            "/rewind <num> - Rewind to message <num>".to_string(),
        ])
    })
}

fn resume_execute(
    args: String,
    _ctx: &mut CommandContext,
) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> {
    Box::pin(async move {
        let arg = args.trim();
        if arg.is_empty() {
            let sessions = crate::business::session::list_sessions().await;
            if sessions.is_empty() {
                return CommandResult::Success("No saved sessions found.\nStart a conversation first, then use /save to create a session.".to_string());
            }
            let mut output = String::from("Recent sessions (use /resume <id> to resume):\n\n");
            for (i, s) in sessions.iter().take(15).enumerate() {
                let summary = s.summary();
                let msg_count = s.messages.len();
                let updated = relative_time(&s.updated_at);
                let title = s.metadata.title.as_deref().unwrap_or("");
                output.push_str(&format!(
                    "  {:>2}. {}  {} msg  {}\n",
                    i + 1,
                    s.id,
                    msg_count,
                    updated,
                ));
                if !title.is_empty() {
                    output.push_str(&format!("      {}\n", title));
                } else if !summary.is_empty() {
                    output.push_str(&format!("      {}\n", summary));
                }
            }
            let remaining = sessions.len().saturating_sub(15);
            if remaining > 0 {
                output.push_str(&format!("\n  ... and {} more sessions\n", remaining));
            }
            CommandResult::Success(output)
        } else {
            CommandResult::Action(CommandAction::ResumeSession(arg.to_string()))
        }
    })
}

/// Format an ISO timestamp as a human-readable relative time string.
fn relative_time(iso: &str) -> String {
    let then = match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => return iso.to_string(),
    };
    let now = chrono::Utc::now();
    let delta = now.signed_duration_since(then);
    if delta.num_seconds() < 60 {
        "just now".to_string()
    } else if delta.num_minutes() < 60 {
        format!("{}m ago", delta.num_minutes())
    } else if delta.num_hours() < 24 {
        format!("{}h ago", delta.num_hours())
    } else if delta.num_days() < 30 {
        format!("{}d ago", delta.num_days())
    } else {
        then.format("%Y-%m-%d").to_string()
    }
}

fn session_execute(
    args: String,
    ctx: &mut CommandContext,
) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> {
    let session_id = ctx.session_id.clone();
    Box::pin(async move {
        let parts: Vec<&str> = args.trim().split_whitespace().collect();
        if parts.is_empty() {
            return CommandResult::Success(format!("Current session: {}", session_id));
        }
        match parts[0] {
            "list" => {
                let sessions = crate::business::session::list_sessions().await;
                if sessions.is_empty() {
                    return CommandResult::Success("No saved sessions".to_string());
                }
                let mut output = String::from("Recent sessions:\n\n");
                for s in sessions.iter().take(15) {
                    let updated = relative_time(&s.updated_at);
                    let summary = s.summary();
                    output.push_str(&format!(
                        "  {}  {} msg  {}\n",
                        s.id,
                        s.messages.len(),
                        updated,
                    ));
                    if !summary.is_empty() {
                        output.push_str(&format!("      {}\n", summary));
                    }
                }
                output.push_str(&format!("\nTotal: {} sessions\n", sessions.len()));
                CommandResult::Success(output)
            }
            "new" => CommandResult::Action(CommandAction::NewSession),
            "rename" => {
                if parts.len() < 3 {
                    return CommandResult::Error("Usage: /session rename <id> <name>".to_string());
                }
                match crate::business::session::update_session_metadata(
                    parts[1],
                    Some(parts[2..].join(" ")),
                    None,
                    None,
                    None,
                )
                .await
                {
                    Ok(_) => CommandResult::Success(format!("Session {} renamed", parts[1])),
                    Err(e) => CommandResult::Error(format!("Failed to rename: {}", e)),
                }
            }
            "delete" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session delete <id>".to_string());
                }
                CommandResult::Confirm {
                    message: format!("Delete session '{}'?", parts[1]),
                    action: ConfirmAction::DeleteSession(parts[1].to_string()),
                }
            }
            "export" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session export <id>".to_string());
                }
                match crate::business::session::load_session(parts[1]).await {
                    Ok(s) => {
                        let export_id = format!("{}-export", parts[1]);
                        let export_session = crate::business::session::Session {
                            id: export_id.clone(),
                            ..s
                        };
                        match crate::business::session::save_session(&export_session).await {
                            Ok(_) => {
                                CommandResult::Success(format!("Exported as session {}", export_id))
                            }
                            Err(e) => CommandResult::Error(format!("Failed to export: {}", e)),
                        }
                    }
                    Err(e) => CommandResult::Error(format!("Failed to export: {}", e)),
                }
            }
            "import" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session import <file>".to_string());
                }
                match tokio::fs::read_to_string(parts[1]).await {
                    Ok(json) => match serde_json::from_str::<crate::business::session::Session>(&json) {
                        Ok(s) => match crate::business::session::save_session(&s).await {
                            Ok(_) => CommandResult::Success(format!(
                                "Imported session {} from {}",
                                s.id, parts[1]
                            )),
                            Err(e) => CommandResult::Error(format!("Failed to import: {}", e)),
                        },
                        Err(e) => CommandResult::Error(format!("Failed to parse session: {}", e)),
                    },
                    Err(e) => CommandResult::Error(format!("Failed to read file: {}", e)),
                }
            }
            _ => CommandResult::Error(format!("Unknown session command: {}", parts[0])),
        }
    })
}

fn rewind_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim();
    if arg.is_empty() {
        CommandResult::Success("Usage: /rewind <number of messages to keep>\nExample: /rewind 5  (keeps only the last 5 messages)".to_string())
    } else if arg.parse::<usize>().is_ok() {
        CommandResult::Action(CommandAction::Compact)
    } else {
        CommandResult::Error("Usage: /rewind <number>".to_string())
    }
}
