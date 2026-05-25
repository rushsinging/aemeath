use super::tools::handle_commit;
use super::PendingImages;
use crate::image::process_image_file;
use kernel::command::{cmd, CommandAction, CommandContext, CommandRegistry, CommandResult};
use kernel::compact;
use kernel::message::Message;
use kernel::session::{self, Session};
use kernel::skill::Skill;
use kernel::state::AppState;
use std::path::Path;
use std::sync::Arc;

pub(crate) enum SlashResult {
    Continue,
    Exit,
    NotFound,
    /// Inject a user message into the conversation and process it with the LLM
    InjectMessage(String),
}

pub(crate) async fn handle_slash_command(
    input: &str,
    messages: &mut Vec<Message>,
    system_prompt: &str,
    context_size: usize,
    total_input: u64,
    total_output: u64,
    total_calls: u64,
    session_id: &str,
    cwd: &Path,
    pending_images: &PendingImages,
    resumed_session: Option<&Session>,
    allow_all: &mut bool,
    skills: &std::collections::HashMap<String, Skill>,
) -> SlashResult {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let cmd = *parts.first().unwrap_or(&"");

    // Helper closures for command matching (match passes &&str)
    let is_exit = |c: &&str| *c == format!("/{}", cmd::EXIT) || *c == format!("/{}", cmd::QUIT);
    let is_clear = |c: &&str| *c == format!("/{}", cmd::CLEAR);
    let is_compact = |c: &&str| *c == format!("/{}", cmd::COMPACT);
    let is_help = |c: &&str| *c == format!("/{}", cmd::HELP);
    let is_usage = |c: &&str| *c == format!("/{}", cmd::USAGE);

    match cmd {
        c if is_exit(&c) => SlashResult::Exit,
        c if is_clear(&c) => {
            messages.clear();
            pending_images.lock().unwrap().clear();
            println!("[conversation cleared]");
            SlashResult::Continue
        }
        c if is_compact(&c) => {
            let (compacted, was_compacted) =
                compact::compact_messages(messages, system_prompt, context_size);
            if was_compacted {
                let old_len = messages.len();
                *messages = compacted;
                println!("[compacted: {} → {} messages]", old_len, messages.len());
            } else {
                println!("[no compaction needed]");
            }
            SlashResult::Continue
        }
        c if is_help(&c) => {
            println!(
                "{}",
                crate::render::StyledText::header("Available Commands")
            );
            println!("{}", crate::render::StyledText::separator());
            println!("  /help     - Show this help message");
            println!("  /exit     - Exit the agent");
            println!("  /quit     - Exit the agent (alias)");
            println!("  /clear    - Clear conversation history");
            println!("  /compact  - Manually compact conversation");
            println!("  /usage    - Show token usage statistics");
            println!("  /context  - Show context window usage");
            println!("  /save     - Save current session to disk");
            println!("  /sessions - List saved sessions");
            println!("  /commit   - Stage all changes and create a git commit");
            println!();
            println!("{}", crate::render::StyledText::header("Image Commands"));
            println!("{}", crate::render::StyledText::separator());
            println!("  /image <path>   - Add an image to the next message");
            println!("  /paste          - Read image from clipboard");
            println!("  /images         - Show pending images");
            println!("  /clear-images   - Clear pending images");
            println!();
            println!("{}", crate::render::StyledText::separator());
            println!(
                "{}",
                crate::render::StyledText::info("Press Ctrl+C to interrupt current request")
            );
            SlashResult::Continue
        }
        c if is_usage(&c) => {
            println!("Usage this session:");
            println!("  API calls: {}", total_calls);
            println!("  Input:     {} tokens", total_input);
            println!("  Output:    {} tokens", total_output);
            println!("  Total:     {} tokens", total_input + total_output);
            SlashResult::Continue
        }
        "/context" => {
            let estimated = compact::estimate_messages_tokens(messages)
                + compact::estimate_tokens(system_prompt);
            let pct = estimated * 100 / context_size.max(1);
            println!(
                "Context window: ~{} / {} tokens ({}%)",
                estimated, context_size, pct
            );
            println!("Messages: {}", messages.len());
            if pct > 80 {
                println!("[auto-compaction will trigger at 80%]");
            }
            SlashResult::Continue
        }
        "/save" => {
            // Use resumed_session if available, otherwise create new session
            let s: Session = if let Some(existing) = resumed_session {
                Session {
                    id: existing.id.clone(),
                    cwd: existing.cwd.clone(),
                    messages: messages.clone(),
                    created_at: existing.created_at.clone(),
                    updated_at: session::now_iso(),
                    metadata: existing.metadata.clone(),
                    tasks: None,
                    workspace: existing.workspace.clone(),
                }
            } else {
                let mut s = Session::new(session_id.to_string(), cwd.to_string_lossy().to_string());
                s.messages = messages.clone();
                s.updated_at = session::now_iso();
                s
            };
            match session::save_session(&s).await {
                Ok(()) => println!("[session saved: {session_id}]"),
                Err(e) => eprintln!("error: {e}"),
            }
            SlashResult::Continue
        }
        "/sessions" => {
            let sessions = session::list_sessions().await;
            if sessions.is_empty() {
                println!("No saved sessions.");
            } else {
                println!("Saved sessions:");
                for (i, s) in sessions.iter().take(10).enumerate() {
                    let msg_count = s.messages.len();
                    println!(
                        "  {}. {} ({} msgs, {})",
                        i + 1,
                        s.id,
                        msg_count,
                        s.updated_at
                    );
                }
                println!("\nResume with: aemeath --resume <session-id>");
            }
            SlashResult::Continue
        }
        "/commit" => {
            handle_commit(cwd).await;
            SlashResult::Continue
        }
        "/image" => {
            let path = parts.get(1);
            if path.is_none() {
                println!("Usage: /image <path>");
                println!("  Add an image file to the next message.");
                println!("  Supported formats: PNG, JPEG, GIF, WebP");
                return SlashResult::Continue;
            }
            let path = path.copied().unwrap_or("");
            // Resolve relative path
            let full_path = if Path::new(path).is_absolute() {
                path.to_string()
            } else {
                cwd.join(path).to_string_lossy().to_string()
            };

            match process_image_file(&full_path).await {
                Ok(img) => {
                    let size = img.original_size;
                    pending_images.lock().unwrap().push(img);
                    println!("[image added ({} bytes)]", size);
                    println!("  Type your message and press Enter to send with the image.");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                }
            }
            SlashResult::Continue
        }
        "/images" => {
            let images = pending_images.lock().unwrap();
            if images.is_empty() {
                println!("No pending images.");
            } else {
                println!("Pending images:");
                for (i, img) in images.iter().enumerate() {
                    println!("  {}. [image {}] ({} bytes)", i + 1, i + 1, img.final_size);
                }
                println!("\nImages will be sent with your next message.");
            }
            SlashResult::Continue
        }
        "/clear-images" => {
            let count = pending_images.lock().unwrap().len();
            pending_images.lock().unwrap().clear();
            println!("[cleared {} pending images]", count);
            SlashResult::Continue
        }
        "/paste" => {
            println!("[reading image from clipboard...]");
            match crate::image::read_clipboard_image().await {
                Ok(img) => {
                    println!("[clipboard image added ({} bytes)]", img.final_size);
                    pending_images.lock().unwrap().push(img);
                    println!("Image queued. Type your message to send it.");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                }
            }
            SlashResult::Continue
        }
        // Try to execute via CommandRegistry
        _ => {
            let cmd_name = cmd.trim_start_matches('/');
            let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();

            // Try to find command in registry
            let registry = CommandRegistry::global();
            if let Some(cmd_obj) = registry.find(cmd_name) {
                let state = AppState::default();
                let config = kernel::config::Config::default();
                let mut ctx = CommandContext::new(
                    Arc::new(state),
                    config,
                    cwd.to_string_lossy().to_string(),
                    session_id.to_string(),
                );

                match cmd_obj.execute(&args, &mut ctx).await {
                    CommandResult::Success(msg) => println!("{}", msg),
                    CommandResult::Error(msg) => eprintln!("error: {}", msg),
                    CommandResult::Action(action) => match action {
                        CommandAction::Exit => return SlashResult::Exit,
                        CommandAction::Clear => {
                            messages.clear();
                            println!("[cleared]");
                        }
                        CommandAction::InjectMessage(prompt) => {
                            println!("[reviewing code changes...]");
                            return SlashResult::InjectMessage(prompt);
                        }
                        CommandAction::ChangeMode(mode) => match mode.as_str() {
                            "ask" => {
                                *allow_all = false;
                                println!("Permission mode set to: ask");
                            }
                            "auto-read" => {
                                *allow_all = false;
                                println!("Permission mode set to: auto-read");
                            }
                            "allow-all" => {
                                *allow_all = true;
                                println!(
                                    "Permission mode set to: allow-all (warning: all tools will be auto-approved)"
                                );
                            }
                            _ => eprintln!("Unknown permission mode: {}", mode),
                        },
                        _ => println!("[action: {:?}]", action),
                    },
                    CommandResult::Confirm { message, .. } => {
                        println!("[confirm: {}]", message);
                    }
                }
                SlashResult::Continue
            } else if let Some(skill) = skills
                .values()
                .find(|s| s.name == cmd_name || s.aliases.iter().any(|a| a == cmd_name))
            {
                // Match skill alias — inject skill content as user message
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                let mut content = skill.content.clone();
                if !args.is_empty() {
                    content = format!("{content}\n\nArguments: {args}");
                }
                println!("[skill: {}]", skill.name);
                SlashResult::InjectMessage(content)
            } else {
                SlashResult::NotFound
            }
        }
    }
}
