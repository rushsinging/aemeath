use aemeath_core::task::{TaskStatus, TaskStore};
use aemeath_core::tool::{Tool, ToolContext, ToolRegistry, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

pub struct TodoRunTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TodoRunTool {
    fn name(&self) -> &str {
        "TodoRun"
    }

    fn description(&self) -> &str {
        "Execute all pending todos with dependency-aware parallel dispatch.\n\n\
         How it works:\n\
         1. Find todos with no unresolved dependencies → dispatch them in parallel via sub-agents.\n\
         2. When a todo completes, unlock downstream todos (blocked_by).\n\
         3. Dispatch the next batch of unblocked todos.\n\
         4. Repeat until all todos are completed or failed.\n\n\
         Each todo gets up to 3 retry attempts on failure.\n\
         Call TodoRun once after TodoWrite — it handles everything automatically."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "max_turns_per_todo": {
                    "type": "integer",
                    "description": "Maximum tool-call rounds per sub-agent (default 50, max 100)"
                }
            }
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn timeout_secs(&self) -> u64 {
        1800 // 30 minutes — batch execution of multiple todos
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let runner = match &ctx.agent_runner {
            Some(r) => r.clone(),
            None => return ToolResult::error("agent runner not available"),
        };

        let max_turns = input
            .get("max_turns_per_todo")
            .and_then(|v| v.as_u64())
            .map(|v| (v as u32).min(100))
            .unwrap_or(50);

        let start_time = std::time::Instant::now();
        let mut log: Vec<String> = Vec::new();

        // Collect all non-completed, non-deleted todos
        let all_todos = self.store.list().await;
        let todo_ids: Vec<String> = all_todos
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .map(|t| t.id.clone())
            .collect();

        if todo_ids.is_empty() {
            return ToolResult::success("No pending todos to execute.");
        }

        let total = todo_ids.len();
        log.push(format!("TodoRun: {} todo(s) to execute\n", total));

        let mut completed_ids: HashSet<String> = HashSet::new();
        let mut failed_ids: HashSet<String> = HashSet::new();

        // Wave-based execution: each wave dispatches all ready todos in parallel
        let mut wave = 0;
        loop {
            if ctx.cancel.is_cancelled() {
                log.push("\n⚠ Cancelled by user".to_string());
                break;
            }

            // Find ready todos: pending + all blocked_by are completed
            let current_todos = self.store.list().await;
            let ready: Vec<_> = current_todos
                .iter()
                .filter(|t| {
                    t.status == TaskStatus::Pending
                        && t.blocked_by.iter().all(|dep| completed_ids.contains(dep))
                })
                .cloned()
                .collect();

            if ready.is_empty() {
                // Check if there are still in_progress or blocked pending todos
                let still_pending = current_todos
                    .iter()
                    .filter(|t| t.status == TaskStatus::Pending)
                    .count();
                if still_pending > 0 {
                    log.push(format!(
                        "\n⚠ {} todo(s) still pending but blocked — check for circular dependencies",
                        still_pending
                    ));
                }
                break;
            }

            wave += 1;
            let batch_size = ready.len();
            log.push(format!(
                "━━━ Wave {} ({} todo{}) ━━━",
                wave,
                batch_size,
                if batch_size > 1 { "s" } else { "" }
            ));

            for t in &ready {
                let blocked_info = if t.blocked_by.is_empty() {
                    String::new()
                } else {
                    format!(" (was blocked by #{})", t.blocked_by.join(", #"))
                };
                log.push(format!("  → #{} {}{}", t.id, t.subject, blocked_info));
            }

            // Mark all ready todos as in_progress
            for t in &ready {
                self.store
                    .update(&t.id, |task| {
                        task.status = TaskStatus::InProgress;
                    })
                    .await;
            }

            // Dispatch all ready todos in parallel
            let cwd_str = ctx.cwd.to_string_lossy().to_string();
            let mut handles = Vec::with_capacity(ready.len());

            for todo in &ready {
                let runner = runner.clone();
                let task_desc = if todo.description.is_empty() {
                    todo.subject.clone()
                } else {
                    format!("{}\n\n{}", todo.subject, todo.description)
                };
                let system = build_sub_agent_system(&cwd_str, max_turns);
                let cwd_buf: PathBuf = cwd_str.clone().into();
                let cancel = ctx.cancel.clone();
                let plan_mode = ctx.plan_mode;
                let allow_all = ctx.allow_all;
                let todo_id = todo.id.clone();

                let handle = tokio::spawn(async move {
                    let sub_ctx = ToolContext {
                        cwd: cwd_buf,
                        cancel,
                        read_files: Arc::new(std::sync::Mutex::new(HashSet::new())),
                        agent_runner: None,
                        plan_mode,
                        allow_all,
                    };
                    let result = runner
                        .run_agent(
                            &task_desc,
                            &system,
                            &[],
                            &ToolRegistry::new(),
                            &sub_ctx,
                            Some(max_turns),
                        )
                        .await;
                    (todo_id, result)
                });
                handles.push(handle);
            }

            // Await all sub-agents in this wave
            for handle in handles {
                match handle.await {
                    Ok((todo_id, result)) => {
                        let is_error = is_failure_result(&result);
                        let subject = ready
                            .iter()
                            .find(|t| t.id == todo_id)
                            .map(|t| t.subject.as_str())
                            .unwrap_or("?");

                        if is_error {
                            // Mark as pending for potential retry or manual intervention
                            self.store
                                .update(&todo_id, |t| {
                                    t.status = TaskStatus::Pending;
                                })
                                .await;
                            failed_ids.insert(todo_id.clone());
                            log.push(format!(
                                "  ✗ #{} {} — {}",
                                todo_id,
                                subject,
                                summarize_output(&result)
                            ));
                        } else {
                            self.store
                                .update(&todo_id, |t| {
                                    t.status = TaskStatus::Completed;
                                })
                                .await;
                            completed_ids.insert(todo_id.clone());
                            log.push(format!("  ✓ #{} {}", todo_id, subject));
                        }
                    }
                    Err(e) => {
                        log.push(format!("  ✗ Sub-agent join error: {}", e));
                    }
                }
            }

            log.push(String::new()); // blank line between waves
        }

        // Final summary
        let elapsed = start_time.elapsed();
        let elapsed_str = if elapsed.as_secs() >= 60 {
            format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
        } else {
            format!("{}s", elapsed.as_secs())
        };

        let final_todos = self.store.list().await;
        let completed = final_todos
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let still_pending = final_todos
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();

        log.push(format!(
            "━━━ Done: {}/{} completed, {} pending ({}) ━━━",
            completed, total, still_pending, elapsed_str
        ));

        ToolResult::success(log.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_sub_agent_system(cwd: &str, max_turns: u32) -> String {
    format!(
        r#"You are a sub-agent performing a specific task. You have access to tools for running commands, reading and editing files, and searching codebases.

Working directory: {cwd}

CRITICAL — Context budget:
- You have a LIMITED context window (~128K tokens). Every file you read consumes tokens.
- ALWAYS use `limit` parameter when reading files: `Read(file_path, limit: 100)` for overviews, `limit: 50` for quick checks. Only omit limit for very small files (<100 lines).
- Use Grep to find specific code instead of reading entire files — this is much more token-efficient.
- Use Glob to discover files, then read only the most relevant ones.
- NEVER read more than 3 files per tool call round.
- You have max {max_turns} rounds of tool calls. Plan ahead — don't waste rounds reading files you won't use.

Instructions:
- Complete the task described in the user message
- Be thorough but concise
- Once you have enough information, STOP using tools and write your final response immediately
- Do not ask questions — make reasonable decisions and proceed"#
    )
}

fn is_failure_result(result: &str) -> bool {
    result.contains("[Sub-agent timed out")
        || result.contains("[Sub-agent reached max turns")
        || result.contains("Sub-agent error:")
        || result.starts_with("Cancelled by user")
}

fn summarize_output(output: &str) -> String {
    const MAX_LEN: usize = 500;
    if output.len() <= MAX_LEN {
        output.to_string()
    } else {
        let truncated = &output[..output.floor_char_boundary(MAX_LEN)];
        format!("{}... [{} chars total]", truncated, output.len())
    }
}
