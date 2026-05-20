use std::path::PathBuf;

use aemeath_core::config::MemoryConfig;
use aemeath_core::hook::HookRunner;
use aemeath_core::memory::{memory_base_dir, project_hash_from_path, MemoryEntry, MemoryStore};

/// System prompt split into a static (cacheable) part and a dynamic (per-session) part.
#[derive(Clone)]
pub struct SystemPromptParts {
    /// Static instructions that rarely change — eligible for prompt caching.
    pub static_part: String,
    /// Dynamic context (date, git status) that changes per session.
    pub dynamic_part: String,
    /// CLAUDE.md content, injected separately as a user-context message.
    pub claude_md: String,
}

fn static_system_prompt_for(cwd_str: &str, is_git: bool) -> String {
    format!(
        r#"You are an interactive agent that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.

# System
 - All text you output outside of tool use is displayed to the user.
 - You can call multiple tools in a single response. If you intend to call multiple tools and there are no dependencies between them, make all independent tool calls in parallel.
 - Do NOT use the Bash to run commands when a relevant dedicated tool is provided:
  - To read files use Read instead of cat, head, tail, or sed
  - To edit files use Edit instead of sed or awk
  - To create files use Write instead of cat with heredoc or echo redirection
  - To search for files use Glob instead of find or ls
  - To search for the content of files, use Grep instead of grep or rg
 - Tool results and user messages may include <system-reminder> tags. These tags contain useful context automatically added by the system.

# Doing tasks
 - In general, do not propose changes to code you haven't read. If a user asks about or wants you to modify a file, read it first.
 - Do not create files unless they're absolutely necessary for achieving your goal.
 - Be careful not to introduce security vulnerabilities such as command injection, XSS, SQL injection.
 - Don't add features, refactor code, or make improvements beyond what was asked.

# Using Agent tool — MANDATORY two-phase approach
Sub-agents have a small context window (~128K tokens) and max 10 tool rounds. They CANNOT review an entire crate or directory.
When a task requires understanding a large codebase (review, refactor, audit, etc.):
 Phase 1 — YOU do the overview:
  - Use Glob to list files
  - Use Read(limit: 30) to skim key files
  - Use Grep to find specific patterns
  - Identify which specific files need deeper analysis
 Phase 2 — Launch FOCUSED agents:
  - Each agent reviews 1-3 SPECIFIC files (give exact paths)
  - Give each agent a SPECIFIC question to answer
  - Do NOT set max_turns unless you have a specific reason — the default (50) works well for most tasks
  - Example: Agent("Review error handling in compact.rs and token_estimation.rs — check edge cases in compaction_urgency and needs_compaction")
 NEVER launch an agent with a vague prompt like "review the core module" or "review all files in X directory".

# Task workflow — MANDATORY
When you use TaskCreate to create tasks, you MUST maintain task status throughout execution:
- For a new multi-step user request, call TaskListCreate before TaskCreate so the task batch has a concise request summary.
- BEFORE starting work on a task yourself with Read/Grep/Glob/Bash/Edit/Write/etc.: call `TaskUpdate(taskId, status="in_progress")` in the same tool batch or an earlier one.
- AFTER completing a task yourself: call `TaskUpdate(taskId, status="completed")` before reporting completion.
- If dispatching a sub-agent for a task: pass `taskId` to the Agent tool and do NOT call TaskUpdate for that task; the dispatcher manages Pending → InProgress → Completed/Pending automatically.
- After all tasks in the current request are completed, call TaskListComplete to close the active task batch.
- Do NOT skip TaskUpdate — task status is visible to the user and must stay accurate.

Use blocked_by to set dependencies: e.g. task 3 depends on task 1 and task 2 completing first.
When the user says "continue", "resume", or similar without specifying a task, call TaskList first to inspect open task batches before choosing work.
System reminders about tasks may refer to older task batches. If a reminder is unrelated to the latest user request, prioritize the latest user request.

BAD:  TaskCreate(3 tasks) → Agent("do task 1") → Agent("do task 2") → Agent("do task 3")  (missing taskId / no lifecycle ownership)
GOOD: TaskListCreate(summary) → TaskCreate(3 tasks) → Agent("do task 1", taskId="1") → TaskUpdate(id2, in_progress) → Bash/Edit for task 2 → TaskUpdate(id2, completed) → TaskListComplete()

# Tone and style
 - Your responses should be short and concise.
 - Do not use emojis unless the user explicitly requests it.

# Environment
 - Working directory: {cwd_str}
 - Is a git repository: {is_git}"#
    )
}

#[cfg(test)]
fn static_system_prompt_for_test(cwd_str: &str, is_git: bool) -> String {
    static_system_prompt_for(cwd_str, is_git)
}

pub async fn build_system_prompt_parts(
    cwd: &PathBuf,
    hook_runner: &HookRunner,
    memory_config: &MemoryConfig,
) -> SystemPromptParts {
    let cwd_str = cwd.to_string_lossy();
    let is_git = is_git_repo(cwd).await;

    // --- Static part: instructions that don't change between sessions ---
    let static_part = static_system_prompt_for(&cwd_str, is_git);

    // --- Dynamic part: session-specific context ---
    let mut dynamic = String::new();

    let date = current_date();
    dynamic.push_str(&format!("# currentDate\nToday's date is {date}."));

    if is_git {
        let git_context = collect_git_context(cwd).await;
        if !git_context.is_empty() {
            dynamic.push_str("\n\n");
            dynamic.push_str(&git_context);
        }
    }

    if let Some(memory_context) = collect_memory_context(cwd, memory_config).await {
        dynamic.push_str("\n\n");
        dynamic.push_str(&memory_context);
    }

    // --- CLAUDE.md: will be injected as a separate user-context message ---
    let claude_md = load_claude_md(cwd, hook_runner).await;

    SystemPromptParts {
        static_part,
        dynamic_part: dynamic,
        claude_md,
    }
}

pub fn current_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let diy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if d < diy {
            break;
        }
        d -= diy;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for dim in &md {
        if d < *dim as i64 {
            break;
        }
        d -= *dim as i64;
        m += 1;
    }
    format!("{:04}-{:02}-{:02}", y, m + 1, d + 1)
}

pub async fn is_git_repo(cwd: &PathBuf) -> bool {
    use tokio::process::Command;
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub async fn load_claude_md(cwd: &PathBuf, hook_runner: &HookRunner) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Check project-level CLAUDE.md
    let project_path = cwd.join("CLAUDE.md");
    if project_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&project_path).await {
            // Trigger InstructionsLoaded hook
            let file_path_str = project_path.to_string_lossy().to_string();
            hook_runner
                .on_instructions_loaded(&file_path_str, "claude_md")
                .await;
            parts.push(content);
        }
    }

    // Check home directory ~/.claude/CLAUDE.md (global instructions)
    if let Some(home) = dirs::home_dir() {
        let global_path = home.join(".claude").join("CLAUDE.md");
        if global_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&global_path).await {
                // Trigger InstructionsLoaded hook
                let file_path_str = global_path.to_string_lossy().to_string();
                hook_runner
                    .on_instructions_loaded(&file_path_str, "claude_md")
                    .await;
                parts.push(content);
            }
        }
    }

    let mut claude_md = parts.join("\n\n");

    // Scan CLAUDE.md for prompt injection
    let warnings = aemeath_core::security::scan_content("CLAUDE.md", &claude_md);
    if !warnings.is_empty() {
        for w in &warnings {
            log::warn!(
                "[Security] {} in {} line {}: {}",
                w.threat_type,
                w.filename,
                w.line_number,
                w.matched_text
            );
        }
        if let Some(prefix) = aemeath_core::security::format_warnings(&warnings) {
            claude_md = format!("{}\n\n{}", prefix, claude_md);
        }
    }

    claude_md
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MemoryContextOptions {
    max_entries: usize,
    max_inject_count: usize,
    similarity_threshold: f64,
}

fn memory_context_options_from_config(config: &MemoryConfig) -> MemoryContextOptions {
    MemoryContextOptions {
        max_entries: config.max_entries,
        max_inject_count: if config.enabled {
            config.max_inject_count
        } else {
            0
        },
        similarity_threshold: config.similarity_threshold,
    }
}

pub async fn collect_memory_context(cwd: &PathBuf, config: &MemoryConfig) -> Option<String> {
    let options = memory_context_options_from_config(config);
    collect_memory_context_with_options(cwd, options).await
}

#[cfg(test)]
async fn collect_memory_context_with_limit(cwd: &PathBuf, limit: usize) -> Option<String> {
    let options = MemoryContextOptions {
        max_entries: 100,
        max_inject_count: limit,
        similarity_threshold: 0.8,
    };
    collect_memory_context_with_options(cwd, options).await
}

async fn collect_memory_context_with_options(
    cwd: &PathBuf,
    options: MemoryContextOptions,
) -> Option<String> {
    if options.max_inject_count == 0 {
        return None;
    }

    let mut store = MemoryStore::new(
        memory_base_dir(),
        project_hash_from_path(cwd),
        options.max_entries,
        options.similarity_threshold,
    )
    .ok()?;
    let entries = store.top_for_inject(options.max_inject_count).ok()?;
    format_memory_context(&entries)
}

fn format_memory_context(entries: &[MemoryEntry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }

    let mut output = String::from("# Project Memory");
    let mut bytes = output.len();
    for entry in entries {
        let line = format!("\n- [{:?}] {}", entry.category, entry.content.trim());
        if bytes + line.len() > 4000 {
            break;
        }
        bytes += line.len();
        output.push_str(&line);
    }

    Some(output)
}

pub async fn collect_git_context(cwd: &PathBuf) -> String {
    use tokio::process::Command;

    let mut parts: Vec<String> = Vec::new();
    parts.push("# Git Context".to_string());

    // Branch name
    if let Ok(output) = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .await
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            parts.push(format!("Current branch: {branch}"));
        }
    }

    // Default branch
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "origin/HEAD"])
        .current_dir(cwd)
        .output()
        .await
    {
        let default = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !default.is_empty() && default != "origin/HEAD" {
            let branch = default.strip_prefix("origin/").unwrap_or(&default);
            parts.push(format!("Default branch: {branch}"));
        }
    }

    // Git user
    if let Ok(output) = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(cwd)
        .output()
        .await
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            parts.push(format!("Git user: {name}"));
        }
    }

    // Status (short)
    if let Ok(output) = Command::new("git")
        .args(["--no-optional-locks", "status", "--short"])
        .current_dir(cwd)
        .output()
        .await
    {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !status.is_empty() {
            let lines: Vec<&str> = status.lines().take(20).collect();
            parts.push(format!("Status:\n{}", lines.join("\n")));
        }
    }

    // Recent commits
    if let Ok(output) = Command::new("git")
        .args(["--no-optional-locks", "log", "--oneline", "-n", "5"])
        .current_dir(cwd)
        .output()
        .await
    {
        let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !log.is_empty() {
            parts.push(format!("Recent commits:\n{log}"));
        }
    }

    let result = parts.join("\n");
    // Truncate to ~2000 bytes, respecting UTF-8 char boundaries
    if result.len() > 2000 {
        let mut end = 2000;
        while end > 0 && !result.is_char_boundary(end) {
            end -= 1;
        }
        result[..end].to_string()
    } else {
        result
    }
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod prompt_tests;
