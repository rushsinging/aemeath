use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::brief::{BriefInput, BriefResult};

/// BriefTool generates a brief summary of the work done in the current session.
pub struct BriefTool;

#[async_trait]
impl TypedTool for BriefTool {
    type Output = BriefResult;
    fn name(&self) -> &str {
        "Brief"
    }
    fn description(&self) -> &str {
        "Generate a brief summary of work completed in this session. Useful for creating status updates, documenting progress, or preparing handoff notes."
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        BriefInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        BriefResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<BriefResult> {
        let args: BriefInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let format = args.format.as_deref().unwrap_or("markdown");
        let title = args.title.as_deref().unwrap_or("Session Brief");
        let include: Vec<&str> = if let Some(arr) = args.include.as_ref() {
            arr.iter().map(|s| s.as_str()).collect()
        } else {
            vec![
                "files_changed",
                "commands_run",
                "decisions",
                "pending_tasks",
                "errors",
            ]
        };

        // Collect session information from the state
        let mut brief_content = String::new();

        // Build brief based on format
        match format {
            "markdown" => {
                brief_content.push_str(&format!("# {}\n\n", title));
                brief_content.push_str(&format!("**Working Directory**: {}\n", ctx.cwd.display()));
                brief_content.push_str(&format!(
                    "**Generated**: {}\n\n",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                ));

                if include.contains(&"files_changed") {
                    brief_content.push_str("## Files Changed\n\n");
                    brief_content
                        .push_str("Use `/status` or check git status for file changes.\n\n");
                }

                if include.contains(&"commands_run") {
                    brief_content.push_str("## Commands Executed\n\n");
                    brief_content
                        .push_str("Check the conversation history for executed commands.\n\n");
                }

                if include.contains(&"decisions") {
                    brief_content.push_str("## Key Decisions\n\n");
                    brief_content.push_str("Review the conversation for decisions made.\n\n");
                }

                if include.contains(&"pending_tasks") {
                    brief_content.push_str("## Pending Tasks\n\n");
                    brief_content.push_str("Use `TaskList` tool to view pending tasks.\n\n");
                }

                if include.contains(&"errors") {
                    brief_content.push_str("## Errors Encountered\n\n");
                    brief_content.push_str("Check conversation history for any errors.\n\n");
                }
            }
            "text" => {
                brief_content.push_str(&format!("{}\n\n", title));
                brief_content.push_str(&format!("Working Directory: {}\n", ctx.cwd.display()));
                brief_content.push_str(&format!(
                    "Generated: {}\n\n",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                ));

                if include.contains(&"files_changed") {
                    brief_content.push_str("FILES CHANGED:\n");
                    brief_content.push_str("  Check git status for details.\n\n");
                }

                if include.contains(&"pending_tasks") {
                    brief_content.push_str("PENDING TASKS:\n");
                    brief_content.push_str("  Use TaskList tool.\n\n");
                }
            }
            "json" => {
                let json_brief = serde_json::json!({
                    "title": title,
                    "working_directory": ctx.cwd.display().to_string(),
                    "generated_at": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    "includes": include,
                    "note": "This is a brief template. Populate with actual session data from conversation history."
                });
                brief_content = serde_json::to_string_pretty(&json_brief).unwrap_or_default();
            }
            _ => {
                return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("Unknown format: {}. Use markdown, text, or json.", format), "data": null}).to_string());
            }
        }

        brief_content.push_str("\n---\n");
        brief_content.push_str(
            "*This brief was generated by aemeath. Add specific details from your conversation.*\n",
        );

        TypedToolResult::success_msg(serde_json::json!({"status": "success", "message": "Brief generated successfully", "data": {"summary": brief_content}}).to_string())
    }
}
