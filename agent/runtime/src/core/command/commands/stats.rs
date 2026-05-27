//! Stats command — show session and usage statistics.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};
use std::future::Future;
use std::pin::Pin;

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new_async(
            "stats".to_string(),
            "Show session and usage statistics".to_string(),
            CommandCategory::Utility,
            stats_execute,
        )
        .with_usage(vec![
            "/stats - Show all statistics".to_string(),
            "/stats session - Session stats".to_string(),
            "/stats tools - Tool usage stats".to_string(),
            "/stats tokens - Token usage estimate".to_string(),
        ])
        .with_aliases(vec!["statistics".to_string()])
    })
}

fn stats_execute(
    args: String,
    ctx: &mut CommandContext,
) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> {
    let session_id = ctx.session_id.clone();
    let model_name = ctx.config.model.name.clone();
    let max_tokens = ctx.config.model.max_tokens;
    let context_size = ctx.config.model.context_size;
    Box::pin(async move {
        let arg = args.trim().to_lowercase();
        match arg.as_str() {
            "" | "all" => {
                let sessions = crate::business::session::list_sessions().await;
                let total_sessions = sessions.len();
                let total_messages: usize = sessions.iter().map(|s| s.messages.len()).sum();
                let output = format!(
                    "Statistics Summary:\n\n\
                     Sessions:\n  Total saved: {}\n  Current session: {}\n  Total messages: {}\n\n\
                     Configuration:\n  Model: {}\n  Max tokens: {}\n  Context size: {}\n\n\
                     Cost Tracking:\n  Use /cost for detailed cost information\n\n\
                     Tokens:\n  Use /stats tokens for token estimate",
                    total_sessions,
                    session_id,
                    total_messages,
                    model_name,
                    max_tokens,
                    context_size
                );
                CommandResult::Success(output)
            }
            "session" => {
                let sessions = crate::business::session::list_sessions().await;
                let mut output = String::from("Session Statistics:\n\n");
                for sess in sessions.iter().take(10) {
                    output.push_str(&format!(
                        "Session {}:\n  Messages: {}\n  Created: {}\n  Updated: {}\n\n",
                        sess.id,
                        sess.messages.len(),
                        sess.created_at,
                        sess.updated_at
                    ));
                }
                if sessions.len() > 10 {
                    output.push_str(&format!("... and {} more sessions\n", sessions.len() - 10));
                }
                CommandResult::Success(output)
            }
            "tools" => CommandResult::Success(
                "Tool Usage Statistics:\n\n\
                     Tool usage tracking is not yet implemented.\n\
                     Future feature: Track which tools are used most frequently.\n\n\
                     Available tools: 27\n\
                     - File operations: Read, Write, Edit, Glob, Grep\n\
                     - Shell: Bash\n\
                     - Tasks: TaskCreate, TaskUpdate, TaskList, TaskGet, TaskStop, TodoWrite\n\
                     - Web: WebFetch, WebSearch\n\
                     - MCP: McpTool, ListMcpResources, ReadMcpResource\n\
                     - Agent: Agent\n\
                     - Plan Mode: EnterPlanMode, ExitPlanMode\n\
                     - Utility: Config, Sleep, AskUserQuestion, ToolSearch, Brief\n\
                     - Skills: Skill\n\
                     - Dev: LSP"
                    .to_string(),
            ),
            "tokens" => {
                let sessions = crate::business::session::list_sessions().await;
                let current_session = sessions.iter().find(|s| s.id == session_id);
                let token_estimate = if let Some(sess) = current_session {
                    share::token_estimation::estimate_messages_tokens(&sess.messages)
                } else {
                    0
                };
                let output = format!(
                    "Token Usage Estimate:\n\n\
                     Current session tokens: ~{}\n\
                     Context size limit: {}\n\
                     Usage: {:.1}%\n\n\
                     Note: This is an estimate based on message content.\n\
                     Actual token counts may vary based on the model's tokenizer.",
                    token_estimate,
                    context_size,
                    (token_estimate as f64 / context_size as f64) * 100.0
                );
                CommandResult::Success(output)
            }
            _ => CommandResult::Error(format!("Unknown stats type: {}", arg)),
        }
    })
}
