use aemeath_core::task::TaskStore;
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

pub struct AgentTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a new agent to handle a focused, scoped task autonomously.\n\nThe Agent tool launches sub-agents that have their own conversation context and can use all available tools. Each sub-agent has a LIMITED context window (~128K tokens) and default 50 tool-call rounds.\n\nTask scoping guidelines:\n- For large tasks (e.g. 'review this project'), use a two-phase approach:\n  Phase 1: YOU (the main agent) do the overview — use Glob/Grep to list files and understand structure.\n  Phase 2: Launch focused agents for specific files or concerns.\n- Give each agent specific file paths and specific questions to answer.\n\nWhen NOT to use the Agent tool:\n- Simple file reads, searches, or single-step tasks — use Read/Grep/Glob directly.\n\nTask coupling (IMPORTANT):\n- If you created tasks with TaskCreate and want to dispatch agents for them, pass the task's ID via `taskId`.\n- When you pass `taskId`, the dispatcher automatically manages task status: Pending → InProgress (when the agent actually starts running, respecting the concurrency limit) → Completed (on success) or Pending (on failure).\n- Do NOT call TaskUpdate to set status=in_progress or status=completed for tasks you've bound to an Agent — the dispatcher owns those transitions. Manual status writes will be overwritten.\n\nUsage notes:\n- Launch multiple agents concurrently for independent tasks.\n- Brief the agent with specific file paths and what to look for.\n- Do NOT set max_turns unless you have a reason — the default (50) works well for most tasks.\n- If you need a short response, say so."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "description": {
                    "type": "string",
                    "description": "A short (3-5 word) description of the task"
                },
                "model": {
                    "type": "string",
                    "enum": ["sonnet", "opus", "haiku"],
                    "description": "Optional model override for this agent"
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum number of tool-call rounds (default 50, max 100)"
                },
                "taskId": {
                    "type": "string",
                    "description": "Optional task ID from TaskCreate. When provided, the dispatcher manages this task's status automatically (InProgress when the agent actually starts, Completed on success). Do NOT manually set task status to in_progress/completed if you pass taskId."
                }
            },
            "required": ["prompt", "description"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true // Sub-agents are independent — safe to run in parallel
    }

    fn timeout_secs(&self) -> u64 {
        600 // 10 minutes — sub-agents run multi-turn LLM conversations
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let prompt = match input.get("prompt").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("missing required parameter: prompt"),
        };

        let _description = match input.get("description").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => return ToolResult::error("missing required parameter: description"),
        };

        // --- Task scope analysis ---
        // Warnings flow through two paths:
        //   1. `scope_hint` in the sub-agent system prompt (below) — so the
        //      sub-agent itself can adjust strategy
        //   2. Prepended to the final ToolResult — so the CALLING agent and
        //      the TUI user both see them as part of the tool's output
        // We deliberately do NOT route these through `log::*` because that
        // would either corrupt the TUI (if stderr) or go to a file nobody
        // reads. Advisory UI messages belong in the UI channel.
        let scope = analyze_task_scope(prompt, &ctx.cwd);

        // Hard block: prompt is far too large for a sub-agent
        if scope.level == ScopeLevel::Block {
              return ToolResult::error(format!(
                  "Task is too large for a sub-agent. Please break it down:\n{}",
                  scope.warnings.join("\n")
              ));
        }

        let _model = input
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("sonnet");

        let max_turns = input
            .get("max_turns")
            .and_then(|v| v.as_u64())
            .map(|v| (v as u32).min(100)); // cap at 100

        let runner = match &ctx.agent_runner {
            Some(r) => r.clone(),
            None => return ToolResult::error("agent runner not available"),
        };

        let cwd_str = ctx.cwd.to_string_lossy();
        let turns = max_turns.unwrap_or(50);

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
- You have max {turns} rounds of tool calls. Plan ahead — don't waste rounds reading files you won't use.{scope_hint}

Instructions:
- Complete the task described in the user message
- Be thorough but concise
- Once you have enough information, STOP using tools and write your final response immediately
- Do not ask questions — make reasonable decisions and proceed"#
        );

        let result = runner
            .run_agent(
                prompt,
                &system,
                &[],
                &aemeath_core::tool::ToolRegistry::new(),
                ctx,
                max_turns,
            )
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
        ToolResult::success(final_output)
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
  /// Task is too large — block and ask the caller to break it down
  Block,
}

struct ScopeResult {
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
          let w = word.trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == ',' || c == ')' || c == ']' || c == ':');
          if w.contains('/') && (w.contains('.') || w.contains("::")) {
              // Only include if it looks like a file path (has a dot extension or is a crate path)
              if w.rsplit('.').next().map(|ext| ext.len() <= 10).unwrap_or(false) {
                  paths.push(w);
              }
          }
      }
  }
  paths
}

/// Broad patterns that indicate the task is too vague or too large.
const LARGE_TASK_PATTERNS: &[&str] = &[
  "entire codebase",
  "whole project",
  "all files",
  "the entire",
  "every file",
  "review everything",
  "review all",
  "audit everything",
  "audit all",
  "refactor everything",
  "refactor all",
];

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
  let existing_paths: Vec<&str> = file_paths.iter().copied().filter(|p| {
      let full = if p.starts_with('/') { Path::new(p).to_path_buf() } else { cwd.join(p) };
      full.exists()
  }).collect();

  if existing_paths.len() > 10 {
      level = ScopeLevel::Block;
      warnings.push(format!(
          "Prompt references {} file paths. A sub-agent cannot effectively process this many files in ~128K tokens. Break into 2-4 smaller agents, each handling a subset of files.",
          existing_paths.len()
      ));
  } else if existing_paths.len() > 5 {
      if level != ScopeLevel::Block {
          level = ScopeLevel::Warn;
      }
      warnings.push(format!(
          "Prompt references {} file paths. This is on the edge of what a sub-agent can handle — consider splitting if files are large.",
          existing_paths.len()
      ));
  }

  // --- 3. Vague / oversized task patterns ---
  for pattern in LARGE_TASK_PATTERNS {
      if prompt_lower.contains(pattern) {
          level = ScopeLevel::Block;
          warnings.push(format!(
              "Task description contains \"{}\" — this is too broad for a sub-agent. Use a two-phase approach: (1) YOU do the overview with Glob/Grep, (2) launch focused agents for specific files.",
              pattern
          ));
          break;
      }
  }

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

  // --- 5. Multiple distinct tasks in one prompt ---
  // Heuristic: count numbered items or bullet points
  let numbered_items = prompt.lines().filter(|l| {
      let trimmed = l.trim();
      trimmed.starts_with("1.") || trimmed.starts_with("2.") || trimmed.starts_with("3.")
          || trimmed.starts_with("4.") || trimmed.starts_with("5.")
  }).count();
  if numbered_items >= 4 {
      if level != ScopeLevel::Block {
          level = ScopeLevel::Warn;
      }
      warnings.push(format!(
          "Prompt contains {}+ numbered tasks. Consider splitting into separate sub-agents, one per task.",
          numbered_items
      ));
  }

  ScopeResult { level, warnings }
}
