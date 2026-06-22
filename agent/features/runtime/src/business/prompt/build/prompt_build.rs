use std::path::{Path, PathBuf};

const INSTRUCTION_SEARCH_DEPTH: u32 = 5;

use hook::api::HookRunner;
use share::config::{paths, MemoryConfig};
use share::i18n::prompt::commit::commit_guidance_template;
use share::i18n::prompt::system::{date_label, static_system_prompt};

use super::git_context::{collect_git_context, is_git_repo};
use crate::LOG_TARGET;

/// System prompt split into a static (cacheable) part and a dynamic (per-session) part.
#[derive(Clone)]
pub struct SystemPromptParts {
    /// Static instructions that rarely change — eligible for prompt caching.
    pub static_part: String,
    /// Dynamic context (date, git status) that changes per session.
    pub dynamic_part: String,
    /// AGENTS.md content, injected separately as a user-context message.
    pub claude_md: String,
}

#[derive(Debug, Clone)]
pub struct PromptContext {
    pub cwd: PathBuf,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
}

impl PromptContext {
    pub fn new(cwd: &Path, provider_name: Option<&str>, model_name: Option<&str>) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            provider_name: provider_name.map(str::to_string),
            model_name: model_name.map(str::to_string),
        }
    }
}

// ---------------------------------------------------------------------------
// Static system prompt — bilingual (EN / ZH)
//
// 文案已迁至 `share::i18n::prompt::system`（项目级 i18n catalog 单一真相）。
// ---------------------------------------------------------------------------

/// Falls back to English for unknown languages.
fn static_system_prompt_for(cwd_str: &str, is_git: bool, lang: &str) -> String {
    static_system_prompt(lang)
        .replace("{cwd_str}", cwd_str)
        .replace("{is_git}", &is_git.to_string())
}

#[cfg(test)]
fn static_system_prompt_for_test(cwd_str: &str, is_git: bool, lang: &str) -> String {
    static_system_prompt_for(cwd_str, is_git, lang)
}

/// Build commit guidance with provider/model trailer. 文案模板迁自 `share::i18n::prompt::commit`。
fn build_commit_guidance(
    provider_name: Option<&str>,
    model_name: Option<&str>,
    lang: &str,
) -> String {
    let provider = provider_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown");
    let model = model_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown");
    let trailer =
        format!("Co-Authored-By: Aemeath ({provider}/{model}) <github:rushsinging/aemeath>");
    commit_guidance_template(lang).replace("{trailer}", &trailer)
}

pub async fn build_system_prompt_parts(
    context: &PromptContext,
    hook_runner: &HookRunner,
    _memory_config: &MemoryConfig,
    lang: &str,
) -> SystemPromptParts {
    let cwd = &context.cwd;
    let cwd_str = cwd.to_string_lossy();
    let is_git = is_git_repo(cwd).await;

    // --- Static part: instructions that don't change between sessions ---
    let static_part = static_system_prompt_for(&cwd_str, is_git, lang);

    // --- Dynamic part: session-specific context ---
    let mut dynamic = String::new();

    let date = current_date();
    let label = date_label(lang);
    dynamic.push_str(&label.replace("{date}", &date));

    dynamic.push_str("\n\n");
    dynamic.push_str(&build_commit_guidance(
        context.provider_name.as_deref(),
        context.model_name.as_deref(),
        lang,
    ));

    if is_git {
        let git_context = collect_git_context(cwd, lang).await;
        if !git_context.is_empty() {
            dynamic.push_str("\n\n");
            dynamic.push_str(&git_context);
        }
    }

    // --- Project instructions: will be injected as a separate user-context message ---
    let claude_md = load_agents_md(cwd, hook_runner, cwd).await;

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

fn project_instruction_walk(cwd: &Path, depth: u32) -> Vec<PathBuf> {
    // 从 cwd 向上 depth 级祖先目录（含 cwd），每层 CLAUDE.md 优先于 AGENTS.md
    paths::project_instruction_dirs(cwd, depth)
        .into_iter()
        .flat_map(|dir| [dir.join(paths::CLAUDE_MD), dir.join(paths::AGENTS_MD)])
        .collect()
}

pub async fn load_agents_md(cwd: &Path, hook_runner: &HookRunner, workspace_root: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Global: ~/.agents/AGENTS.md first, then fallback to ~/.claude/CLAUDE.md
    let global_paths = [
        paths::global_agents_md_path(),
        paths::old_global_claude_md_path(),
    ];
    for global_path in &global_paths {
        if global_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(global_path).await {
                let file_path_str = global_path.to_string_lossy().to_string();
                hook_runner
                    .on_instructions_loaded(&file_path_str, "agents_md", workspace_root)
                    .await;
                parts.push(content);
            }
            break;
        }
    }

    // Project: walk up INSTRUCTION_SEARCH_DEPTH levels, Claude-first at each level
    for project_path in project_instruction_walk(cwd, INSTRUCTION_SEARCH_DEPTH) {
        if project_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&project_path).await {
                let file_path_str = project_path.to_string_lossy().to_string();
                hook_runner
                    .on_instructions_loaded(&file_path_str, "agents_md", workspace_root)
                    .await;
                parts.push(content);
            }
            break;
        }
    }

    let mut agents_md = parts.join("\n\n");

    let warnings = policy::api::scan_content("AGENTS.md", &agents_md);
    if !warnings.is_empty() {
        for w in &warnings {
            log::warn!(target: LOG_TARGET,
                "[Security] {} in {} line {}: {}",
                w.threat_type,
                w.filename,
                w.line_number,
                w.matched_text
            );
        }
        if let Some(prefix) = policy::api::format_warnings(&warnings) {
            agents_md = format!("{}\n\n{}", prefix, agents_md);
        }
    }

    agents_md
}

#[cfg(test)]
#[path = "prompt_build_tests.rs"]
mod prompt_build_tests;
