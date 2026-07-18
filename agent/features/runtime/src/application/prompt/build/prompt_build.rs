use std::path::{Path, PathBuf};

const INSTRUCTION_SEARCH_DEPTH: u32 = 5;

use hook::api::HookRunner;
use share::config::paths;
use share::i18n::prompt::commit::commit_guidance_template;
use share::i18n::prompt::system::{date_label, static_system_prompt};

use super::git_context::{collect_git_context, is_git_repo};

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
    // 从最远祖先到 cwd，每层 AGENTS.md 在 CLAUDE.md 前，保证具体规则最后注入。
    let mut dirs = paths::project_instruction_dirs(cwd, depth);
    dirs.reverse();
    dirs.into_iter()
        .flat_map(|dir| [dir.join(paths::AGENTS_MD), dir.join(paths::CLAUDE_MD)])
        .collect()
}

#[derive(Debug)]
struct UserGuidanceFile {
    path: PathBuf,
    content: String,
}

async fn read_user_guidance_files(
    paths: &[PathBuf],
    hook_runner: &HookRunner,
    workspace_root: &Path,
) -> Vec<UserGuidanceFile> {
    let mut files = Vec::new();
    for path in paths {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            hook_runner
                .on_instructions_loaded(&path.to_string_lossy(), "agents_md", workspace_root)
                .await;
            files.push(UserGuidanceFile {
                path: path.clone(),
                content,
            });
        }
    }
    files
}

fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_user_guidance(files: &[UserGuidanceFile]) -> String {
    files
        .iter()
        .map(|file| {
            let source = escape_xml_attribute(&file.path.to_string_lossy());
            format!(
                "<guidance source=\"{}\">\n{}\n</guidance>",
                source, file.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn load_agents_md_from_paths(
    global_paths: &[PathBuf],
    project_paths: &[PathBuf],
    hook_runner: &HookRunner,
    workspace_root: &Path,
) -> String {
    let mut files = read_user_guidance_files(global_paths, hook_runner, workspace_root).await;
    files.extend(read_user_guidance_files(project_paths, hook_runner, workspace_root).await);

    // 去重：CLAUDE.md 常是 AGENTS.md 的软链，worktree 路径也会导致同一文件被遍历多次。
    // 先按 canonicalize 后的真实路径去重，再按内容去重（兜底不同路径但内容完全相同的情况）。
    let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut seen_contents: std::collections::HashSet<String> = std::collections::HashSet::new();
    files.retain(|file| {
        let canonical = std::fs::canonicalize(&file.path).unwrap_or_else(|_| file.path.clone());
        let path_ok = seen_paths.insert(canonical);
        let content_ok = seen_contents.insert(file.content.clone());
        path_ok && content_ok
    });

    scan_user_guidance(render_user_guidance(&files))
}

fn scan_user_guidance(user_guidance: String) -> String {
    let assessment = context::guidance::assess_guidance("AGENTS.md", &user_guidance);
    for warning in &assessment.warnings {
        log::warn!(target: crate::LOG_TARGET,
            "[Security] {} in {} line {}: {}",
            warning.threat_type,
            warning.filename,
            warning.line_number,
            warning.matched_text
        );
    }

    assessment.content
}

pub async fn load_agents_md(cwd: &Path, hook_runner: &HookRunner, workspace_root: &Path) -> String {
    let global_paths = [
        paths::global_agents_md_path(),
        paths::old_global_claude_md_path(),
    ];
    let project_paths = project_instruction_walk(cwd, INSTRUCTION_SEARCH_DEPTH);

    load_agents_md_from_paths(&global_paths, &project_paths, hook_runner, workspace_root).await
}

#[cfg(test)]
#[path = "prompt_build_tests.rs"]
mod prompt_build_tests;
