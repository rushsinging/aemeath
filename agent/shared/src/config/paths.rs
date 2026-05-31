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
pub const MCP_CONFIG_FILE: &str = "mcp.json";
pub const HISTORY_FILE: &str = "history.json";
pub const COST_HISTORY_FILE: &str = "cost_history.json";
pub const SETTINGS_FILE: &str = "settings.json";

pub fn global_agents_dir() -> PathBuf {
    PathBuf::from(AGENTS_DIR_NAME)
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
        assert_eq!(global_config_path(), PathBuf::from(".agents/aemeath.json"));
        assert_eq!(global_agents_md_path(), PathBuf::from(".agents/AGENTS.md"));
        assert_eq!(global_skills_dir(), PathBuf::from(".agents/skills"));
        assert_eq!(global_logs_dir(), PathBuf::from(".agents/logs"));
        assert_eq!(global_guidance_dir(), PathBuf::from(".agents/guidance"));
        assert_eq!(global_memory_dir(), PathBuf::from(".agents/memory"));
        assert_eq!(global_sessions_dir(), PathBuf::from(".agents/sessions"));
        assert_eq!(global_hooks_dir(), PathBuf::from(".agents/hooks"));
        assert_eq!(global_mcp_config_path(), PathBuf::from(".agents/mcp.json"));
        assert_eq!(global_history_path(), PathBuf::from(".agents/history.json"));
        assert_eq!(
            global_cost_history_path(),
            PathBuf::from(".agents/cost_history.json")
        );
        assert_eq!(
            global_settings_path(),
            PathBuf::from(".agents/settings.json")
        );
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
