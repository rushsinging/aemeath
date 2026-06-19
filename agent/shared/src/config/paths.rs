//! Codex 风格配置路径。

use std::path::{Path, PathBuf};

pub const AGENTS_DIR_ENV: &str = "AEMEATH_AGENTS_DIR";
pub const NEW_CONFIG_FILE: &str = "aemeath.json";
pub const OLD_CONFIG_FILE: &str = "config.json";
pub const AGENTS_MD: &str = "AGENTS.md";
pub const CLAUDE_MD: &str = "CLAUDE.md";
pub const AGENTS_DIR_NAME: &str = ".agents";
pub const CLAUDE_DIR_NAME: &str = ".claude";
pub const OLD_AEMEATH_DIR_NAME: &str = ".aemeath";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const LOGS_DIR_NAME: &str = "logs";
pub const GUIDANCE_DIR_NAME: &str = "guidance";
pub const MEMORY_DIR_NAME: &str = "memory";
pub const SESSIONS_DIR_NAME: &str = "sessions";
pub const HOOKS_DIR_NAME: &str = "hooks";
pub const TOOL_OUTPUTS_DIR_NAME: &str = "tool_outputs";
pub const MCP_CONFIG_FILE: &str = "mcp.json";
pub const HISTORY_FILE: &str = "history.json";
pub const COST_HISTORY_FILE: &str = "cost_history.json";
pub const SETTINGS_FILE: &str = "settings.json";

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
    PathBuf::from(CLAUDE_DIR_NAME).join(CLAUDE_MD)
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

/// `~/.agents/tool_outputs/` — 超长工具结果的落盘根目录。
pub fn global_tool_outputs_dir() -> PathBuf {
    global_agents_dir().join(TOOL_OUTPUTS_DIR_NAME)
}

/// `~/.agents/tool_outputs/{session_id}/` — 某个 session 的工具结果子目录。
///
/// 生命周期与 session 绑定：session 删除时一并清理。
pub fn session_tool_outputs_dir(session_id: &str) -> PathBuf {
    global_tool_outputs_dir().join(session_id)
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

pub fn global_cost_history_path() -> PathBuf {
    global_agents_dir().join(COST_HISTORY_FILE)
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
mod tests {
    use super::*;

    /// 测试用环境变量守护：构造时设值，析构时还原。
    /// 避免全局 env 污染其它测试。
    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 测试用唯一序号（避免读时钟——shared kernel 禁用 SystemTime::now）。
    static UNIQUE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct TestEnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TestEnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let guard = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
            let old = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                old,
                _guard: guard,
            }
        }

        fn unset(key: &'static str) -> Self {
            let guard = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
            let old = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self {
                key,
                old,
                _guard: guard,
            }
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(old) = &self.old {
                    std::env::set_var(self.key, old);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn test_project_paths_use_agents_directory() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(
            project_config_path(&cwd),
            PathBuf::from("/tmp/demo/.agents/aemeath.json")
        );
        assert_eq!(
            project_agents_md_path(&cwd),
            PathBuf::from("/tmp/demo/AGENTS.md")
        );
        assert_eq!(
            old_project_claude_md_path(&cwd),
            PathBuf::from("/tmp/demo/CLAUDE.md")
        );
        assert_eq!(
            project_claude_settings_path(&cwd),
            PathBuf::from("/tmp/demo/.claude/settings.json")
        );
        assert_eq!(
            project_claude_skills_dir(&cwd),
            PathBuf::from("/tmp/demo/.claude/skills")
        );
        assert_eq!(
            project_skills_dir(&cwd),
            PathBuf::from("/tmp/demo/.agents/skills")
        );
    }

    #[test]
    fn test_global_data_paths_use_agents_directory() {
        // 用 env 隔离，避免污染真实 home/.agents
        let temp_agents_dir = std::env::temp_dir().join(format!(
            "aemeath_shared_paths_{}",
            UNIQUE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, &temp_agents_dir);

        assert_eq!(global_config_path(), temp_agents_dir.join("aemeath.json"));
        assert_eq!(global_agents_md_path(), temp_agents_dir.join("AGENTS.md"));
        assert_eq!(global_skills_dir(), temp_agents_dir.join("skills"));
        assert_eq!(global_logs_dir(), temp_agents_dir.join("logs"));
        assert_eq!(global_guidance_dir(), temp_agents_dir.join("guidance"));
        assert_eq!(global_memory_dir(), temp_agents_dir.join("memory"));
        assert_eq!(global_sessions_dir(), temp_agents_dir.join("sessions"));
        assert_eq!(global_hooks_dir(), temp_agents_dir.join("hooks"));
        assert_eq!(global_mcp_config_path(), temp_agents_dir.join("mcp.json"));
        assert_eq!(global_history_path(), temp_agents_dir.join("history.json"));
        assert_eq!(
            global_cost_history_path(),
            temp_agents_dir.join("cost_history.json")
        );
        assert_eq!(
            global_settings_path(),
            temp_agents_dir.join("settings.json")
        );
    }

    #[test]
    fn test_global_agents_dir_falls_back_to_home() {
        // 无 env 时必须落到 home/.agents（而非相对路径 .agents）
        let _guard = TestEnvGuard::unset(AGENTS_DIR_ENV);
        let expected = home_dir_or_dot().join(AGENTS_DIR_NAME);
        assert_eq!(global_agents_dir(), expected);
    }

    #[test]
    fn test_expand_home() {
        let home = home_dir_or_dot();
        assert_eq!(expand_home(Path::new("~")), home);
        assert_eq!(expand_home(Path::new("~/foo/bar")), home.join("foo/bar"));
        // 非 ~ 前缀原样返回
        assert_eq!(
            expand_home(Path::new("/abs/path")),
            PathBuf::from("/abs/path")
        );
        assert_eq!(
            expand_home(Path::new("relative")),
            PathBuf::from("relative")
        );
    }

    #[test]
    fn test_global_agents_dir_env_empty_string_falls_back_to_home() {
        // env 设了但为空，应回退到 home/.agents（而非空路径）
        let _guard = TestEnvGuard::set(AGENTS_DIR_ENV, "   ");
        let expected = home_dir_or_dot().join(AGENTS_DIR_NAME);
        assert_eq!(global_agents_dir(), expected);
    }

    #[test]
    fn test_project_instruction_dirs_includes_cwd_and_ancestors() {
        let cwd = PathBuf::from("/a/b/c/d");
        let dirs = project_instruction_dirs(&cwd, 2);
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/a/b/c/d"),
                PathBuf::from("/a/b/c"),
                PathBuf::from("/a/b"),
            ]
        );
    }

    #[test]
    fn test_project_instruction_dirs_depth_zero_cwd_only() {
        let cwd = PathBuf::from("/a/b");
        let dirs = project_instruction_dirs(&cwd, 0);
        assert_eq!(dirs, vec![PathBuf::from("/a/b")]);
    }

    #[test]
    fn test_old_project_paths_use_aemeath_and_claude() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(
            old_project_config_path(&cwd),
            PathBuf::from("/tmp/demo/.aemeath/config.json")
        );
        assert_eq!(
            old_project_claude_md_path(&cwd),
            PathBuf::from("/tmp/demo/CLAUDE.md")
        );
        assert_eq!(
            project_claude_settings_path(&cwd),
            PathBuf::from("/tmp/demo/.claude/settings.json")
        );
        assert_eq!(
            project_claude_skills_dir(&cwd),
            PathBuf::from("/tmp/demo/.claude/skills")
        );
        assert_eq!(
            old_project_skills_dir(&cwd),
            PathBuf::from("/tmp/demo/.aemeath/skills")
        );
    }
}
