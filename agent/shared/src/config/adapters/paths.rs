//! Codex 风格配置路径。

use std::path::{Path, PathBuf};

pub use super::constants::{
    AGENTS_DIR_ENV, AGENTS_DIR_NAME, AGENTS_MD, CLAUDE_DIR_NAME, CLAUDE_MD, GUIDANCE_DIR_NAME,
    HISTORY_FILE, HOOKS_DIR_NAME, LOGS_DIR_NAME, MCP_CONFIG_FILE, MEMORY_DIR_NAME, NEW_CONFIG_FILE,
    OLD_AEMEATH_DIR_NAME, OLD_CONFIG_FILE, SESSIONS_DIR_NAME, SETTINGS_FILE, SKILLS_DIR_NAME,
    TOOL_RESULTS_DIR_NAME, WORKTREES_DIR_NAME,
};

/// 解析 home 目录（读取 `$HOME`）。
///
/// 不依赖 `dirs` crate，以满足 shared kernel 零外部行为依赖约束
///（见 `check-share-minimal-kernel.sh` 依赖白名单）。Unix 下 `$HOME`
/// 始终设置；项目仅发布 macOS/Linux 二进制。
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn home_dir_or_dot() -> PathBuf {
    home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn global_agents_dir() -> PathBuf {
    if let Ok(value) = std::env::var(AGENTS_DIR_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return expand_home(Path::new(trimmed));
        }
    }

    home_dir_or_dot().join(AGENTS_DIR_NAME)
}

/// 展开 `~` / `~/` 前缀为 home 目录。
///
/// - `~` → home
/// - `~/foo` → home/foo
/// - 其它原样返回
pub fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return home_dir_or_dot();
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

pub fn global_config_path() -> PathBuf {
    global_agents_dir().join(NEW_CONFIG_FILE)
}

pub fn old_global_config_path() -> PathBuf {
    PathBuf::from(OLD_AEMEATH_DIR_NAME).join(OLD_CONFIG_FILE)
}

pub fn project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(AGENTS_DIR_NAME).join(NEW_CONFIG_FILE)
}

pub fn old_project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(OLD_AEMEATH_DIR_NAME).join(OLD_CONFIG_FILE)
}

pub fn global_agents_md_path() -> PathBuf {
    global_agents_dir().join(AGENTS_MD)
}

pub fn old_global_claude_md_path() -> PathBuf {
    home_dir_or_dot().join(CLAUDE_DIR_NAME).join(CLAUDE_MD)
}

pub fn project_agents_md_path(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_MD)
}

pub fn old_project_claude_md_path(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_MD)
}

pub fn project_claude_settings_path(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_DIR_NAME).join(SETTINGS_FILE)
}

/// 从 `cwd` 向上探索 `depth` 级祖先目录（含 `cwd` 自身），返回目录路径列表。
///
/// 纯路径拼接，无 fs IO。用于项目指令搜索（`load_agents_md`）与
/// config reload snapshot 监控共享同一套目录发现逻辑。
/// 返回顺序：`[cwd, parent, grandparent, ...]`，共 `depth + 1` 个元素。
pub fn project_instruction_dirs(cwd: &Path, depth: u32) -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(depth as usize + 1);
    let mut current = Some(cwd);
    for _ in 0..=depth {
        match current {
            Some(dir) => {
                dirs.push(dir.to_path_buf());
                current = dir.parent();
            }
            None => break,
        }
    }
    dirs
}

pub fn project_claude_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(CLAUDE_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn global_skills_dir() -> PathBuf {
    global_agents_dir().join(SKILLS_DIR_NAME)
}

pub fn global_logs_dir() -> PathBuf {
    global_agents_dir().join(LOGS_DIR_NAME)
}

pub fn global_guidance_dir() -> PathBuf {
    global_agents_dir().join(GUIDANCE_DIR_NAME)
}

pub fn global_memory_dir() -> PathBuf {
    global_agents_dir().join(MEMORY_DIR_NAME)
}

pub fn global_sessions_dir() -> PathBuf {
    global_agents_dir().join(SESSIONS_DIR_NAME)
}

/// `~/.agents/worktrees/` — EnterWorktree 默认创建 worktree 的根目录。
pub fn global_worktrees_dir() -> PathBuf {
    global_agents_dir().join(WORKTREES_DIR_NAME)
}

/// `~/.agents/tool-results/` — 超长工具结果的落盘根目录。
pub fn global_tool_results_dir() -> PathBuf {
    global_agents_dir().join(TOOL_RESULTS_DIR_NAME)
}

/// `~/.agents/tool-results/{session_id}/` — 某个 session 的工具结果子目录。
///
/// 工具结果按 session ID 归档，但删除 Session 不级联删除历史工具结果。
pub fn session_tool_results_dir(session_id: &str) -> PathBuf {
    global_tool_results_dir().join(session_id)
}

pub fn global_hooks_dir() -> PathBuf {
    global_agents_dir().join(HOOKS_DIR_NAME)
}

pub fn global_mcp_config_path() -> PathBuf {
    global_agents_dir().join(MCP_CONFIG_FILE)
}

pub fn global_history_path() -> PathBuf {
    global_agents_dir().join(HISTORY_FILE)
}

pub fn global_settings_path() -> PathBuf {
    global_agents_dir().join(SETTINGS_FILE)
}

pub fn old_global_skills_dir() -> PathBuf {
    PathBuf::from(OLD_AEMEATH_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(AGENTS_DIR_NAME).join(SKILLS_DIR_NAME)
}

pub fn old_project_skills_dir(cwd: &Path) -> PathBuf {
    cwd.join(OLD_AEMEATH_DIR_NAME).join(SKILLS_DIR_NAME)
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
