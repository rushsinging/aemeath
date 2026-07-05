use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::agent::{AgentInput, AgentResult};
use std::path::Path;
use std::sync::Arc;
use storage::api::{TaskStatus, TaskStore};

const SUB_AGENT_MAX_TURNS_CAP: u32 = 1000;

pub struct AgentTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl TypedTool for AgentTool {
    type Output = AgentResult;
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a new agent to handle a focused, scoped task autonomously.\n\nEach sub-agent has its own context (~128K tokens, up to 1000 rounds) and can use all tools. Multiple Agent calls in the SAME response run concurrently. Pass `taskId` to bind to a tracked task for automatic status management."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::agent(lang))
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        AgentInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        AgentResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true // Sub-agents are independent — safe to run in parallel
    }

    fn timeout_secs(&self) -> u64 {
        1800 // 30 minutes — sub-agents run multi-turn LLM conversations
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<AgentResult> {
        let args: AgentInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };

        if args.prompt.is_empty() {
            return TypedToolResult::error("missing required parameter: prompt");
        }
        if args.description.is_empty() {
            return TypedToolResult::error("missing required parameter: description");
        }
        let prompt = args.prompt.as_str();

        // --- Task scope analysis ---
        // Warnings flow through two paths:
        //   1. `scope_hint` in the sub-agent system prompt (below) — so the
        //      sub-agent itself can adjust strategy
        //   2. Prepended to the final ToolResult — so the CALLING agent and
        //      the TUI user both see them as part of the tool's output
        // We deliberately do NOT route these through `log::*` because that
        // would either corrupt the TUI (if stderr) or go to a file nobody
        // reads. Advisory UI messages belong in the UI channel.
        let cwd = ctx.workspace_read().current_path_base();
        let scope = analyze_task_scope(prompt, &cwd);

        // Scope analysis may produce warnings but never blocks — the calling
        // agent is responsible for deciding whether the task is appropriate.
        // (Previously, ScopeLevel::Block returned an error here, which was too
        // aggressive for expert users who know what they're doing.)

        let max_turns = args
            .max_turns
            .map(|v| (v as u32).min(SUB_AGENT_MAX_TURNS_CAP))
            .unwrap_or(SUB_AGENT_MAX_TURNS_CAP);
        let runner = match &ctx.resources.agent_runner {
            Some(r) => r.clone(),
            None => return TypedToolResult::error("agent runner not available"),
        };

        let cwd_str = cwd.to_string_lossy();
        let turns = max_turns;

        // Resolve role and model:
        //   - `model` takes precedence: a direct "provider/model_id" spec
        //   - `role` is resolved by CliAgentRunner via AgentsConfig::roles
        //   - If neither is set, the runner uses the default model
        let role = args.role.as_deref();
        let model = args.model.as_deref();
        let model_spec = if model.is_some() { model } else { role };

        // --- Task status management ---
        // If `taskId` is provided, automatically manage its lifecycle:
        //   Pending → InProgress (before run_agent)
        //   InProgress → Completed (on success) or Pending (on failure)
        let task_id = args.taskId.clone();

        if let Some(ref tid) = task_id {
            if self.store.get(tid).await.is_none() {
                return TypedToolResult::error(format!("task not found: {tid}"));
            }
            self.store
                .update(tid, |t| {
                    t.status = TaskStatus::InProgress;
                })
                .await;
        }
        // Prepend scope warnings as guidance so the sub-agent can adjust its strategy
        let scope_hint = if scope.warnings.is_empty() {
            String::new()
        } else {
            format!(
                "\n\n# Scope Warnings (from pre-launch analysis)\nThe following concerns were detected about task size. Adjust your strategy accordingly:\n{}\n",
                scope.warnings.iter().map(|w| format!("- {w}")).collect::<Vec<_>>().join("\n")
            )
        };

        let system = format!(
            r#"You are a sub-agent performing a specific task. You have access to tools for running commands, reading and editing files, and searching codebases.

Working directory: {cwd_str}

CRITICAL — Context budget:
- You have a LIMITED context window (~128K tokens). Every file you read consumes tokens.
- ALWAYS use `limit` parameter when reading files: `Read(file_path, limit: 100)` for overviews, `limit: 50` for quick checks. Only omit limit for very small files (<100 lines).
- Use Grep to find specific code instead of reading entire files — this is much more token-efficient.
- Use Glob to discover files, then read only the most relevant ones.
- NEVER read more than 3 files per tool call round.
- You have max {turns} rounds of tool calls. Plan ahead — don't waste rounds reading files you won't use.
- You cannot use Task*, AskUserQuestion, or Agent tools. Task tracking and user clarification belong to the parent agent.
- If the task is ambiguous or needs user input, return a concise `blocked:` explanation instead of asking the user.{scope_hint}

Instructions:- Complete the task described in the user message
- Be thorough but concise
- Once you have enough information, STOP using tools and write your final response immediately
- Do not ask questions — make reasonable decisions and proceed"#
        );

        let result = runner
            .run_agent(crate::api::AgentRunRequest {
                prompt,
                system: &system,
                ctx,
                max_turns,
                model_spec,
                progress_tx: ctx.progress_tx.clone(),
            })
            .await;

        // Prepend scope warnings to the result so the caller (and the user
        // watching the TUI) sees them alongside the sub-agent's output.
        let final_output = if scope.warnings.is_empty() {
            result
        } else {
            let banner = scope
                .warnings
                .iter()
                .map(|w| format!("⚠ {w}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("[scope warnings]\n{banner}\n\n{result}")
        };

        // Update task status based on agent outcome
        if let Some(ref tid) = task_id {
            let is_failure = is_agent_failure(&final_output);
            self.store
                .update(tid, |t| {
                    t.status = if is_failure {
                        TaskStatus::Pending
                    } else {
                        TaskStatus::Completed
                    };
                })
                .await;
        }

        // text → 父 LLM：必须包含子代理实际产出，否则父 agent 无法基于结果决策。
        // data → TUI：结构化展示（AgentResult.output 同步保留）。
        let text = if final_output.trim().is_empty() {
            "子代理执行完成（无输出）".to_string()
        } else {
            final_output.clone()
        };
        TypedToolResult::success(
            text,
            AgentResult {
                task_id,
                output: final_output,
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Task scope analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScopeLevel {
    /// Task looks well-scoped
    Ok,
    /// Task has warning signs — let it proceed but inject hints
    Warn,
}

struct ScopeResult {
    #[allow(dead_code)]
    level: ScopeLevel,
    warnings: Vec<String>,
}

/// Extract file path-like tokens from the prompt text.
/// Matches patterns like `src/foo.rs`, `./bar.ts`, `/absolute/path`, `path/to/file.ext`.
fn extract_file_paths(prompt: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    // Match sequences containing a '/' and ending with a typical source extension or config extension
    for line in prompt.lines() {
        for word in line.split_whitespace() {
            // Strip surrounding punctuation like brackets, parens, backticks, quotes, commas
            let w = word.trim_matches(|c: char| {
                c == '`' || c == '"' || c == '\'' || c == ',' || c == ')' || c == ']' || c == ':'
            });
            if w.contains('/') && (w.contains('.') || w.contains("::")) {
                // Only include if it looks like a file path (has a dot extension or is a crate path)
                if w.rsplit('.')
                    .next()
                    .map(|ext| ext.len() <= 10)
                    .unwrap_or(false)
                {
                    paths.push(w);
                }
            }
        }
    }
    paths
}

/// Patterns that indicate the task might be vague or large.
/// These now only produce warnings — they no longer block execution.
/// Kept as an empty array for documentation purposes; broad keyword
/// matching has been removed — the calling agent decides scope.
#[allow(dead_code)]
const LARGE_TASK_PATTERNS: &[&str] = &[];

/// Patterns that indicate the task might be single-step and doesn't need a sub-agent.
const SIMPLE_TASK_PATTERNS: &[&str] = &[
    "read the file",
    "show me the content",
    "what does this file do",
    "cat the file",
    "print the file",
];

fn analyze_task_scope(prompt: &str, cwd: &Path) -> ScopeResult {
    let mut warnings = Vec::new();
    let mut level = ScopeLevel::Ok;

    let prompt_lower = prompt.to_lowercase();

    // --- 1. Prompt size check ---
    // Very long prompt consumes too much of the sub-agent's context window
    // No size limit on prompt — the calling agent may need to pass full file contents

    // --- 2. File path enumeration ---
    let file_paths = extract_file_paths(prompt);
    let existing_paths: Vec<&str> = file_paths
        .iter()
        .copied()
        .filter(|p| {
            let full = if p.starts_with('/') {
                Path::new(p).to_path_buf()
            } else {
                cwd.join(p)
            };
            full.exists()
        })
        .collect();

    if existing_paths.len() > 10 {
        level = ScopeLevel::Warn;
        warnings.push(format!(
          "Prompt references {} file paths. A sub-agent may not effectively process this many files in ~128K tokens. Consider breaking into 2-4 smaller agents, each handling a subset of files.",
          existing_paths.len()
      ));
    } else if existing_paths.len() > 5 {
        if level != ScopeLevel::Warn {
            level = ScopeLevel::Warn;
        }
        warnings.push(format!(
          "Prompt references {} file paths. This is on the edge of what a sub-agent can handle — consider splitting if files are large.",
          existing_paths.len()
      ));
    }

    // --- 3. Vague / oversized task patterns ---
    // LARGE_TASK_PATTERNS is now empty — broad keyword checks removed.
    // The calling agent is responsible for deciding task appropriateness.

    // --- 4. Simple task that doesn't need a sub-agent ---
    for pattern in SIMPLE_TASK_PATTERNS {
        if prompt_lower.contains(pattern) {
            if level == ScopeLevel::Ok {
                level = ScopeLevel::Warn;
            }
            warnings.push(format!(
              "This looks like a simple task (\"{}\"). Use Read/Grep/Glob directly instead of launching a sub-agent.",
              pattern
          ));
            break;
        }
    }

    ScopeResult { level, warnings }
}

/// Detect whether a sub-agent result indicates failure (error, timeout,
/// cancellation, or max-turns exhaustion). These markers are produced by
/// `CliAgentRunner::run_agent` in `agent_runner.rs`.
fn is_agent_failure(result: &str) -> bool {
    result.contains("Cancelled by user")
        || result.contains("[Sub-agent timed out")
        || result.contains("Sub-agent error:")
        || result.contains("[Sub-agent reached max turns")
}

#[cfg(test)]
#[path = "agent_tool_tests.rs"]
mod agent_tool_tests;
