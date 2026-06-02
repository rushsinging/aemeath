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

/// Maximum directory depth to search up and down from cwd for project instructions.
pub const INSTRUCTION_SEARCH_DEPTH: u32 = 5;

/// Return all candidate paths for project-level instruction files (CLAUDE.md, AGENTS.md)
/// by walking up to `depth` ancestor directories and down `depth` levels of subdirectories
/// from `cwd`. Claude-first ordering is preserved at each level.
pub fn project_instruction_walk(cwd: &Path, depth: u32) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. Walk upward from cwd (inclusive)
    let mut current = Some(cwd);
    for _ in 0..=depth {
        if let Some(dir) = current {
            push_instruction_paths_for_dir(&mut paths, dir, depth);
            current = dir.parent();
        } else {
            break;
        }
    }

    paths
}

/// For a given directory, push CLAUDE.md and AGENTS.md for the dir itself,
/// then recurse down up to `remaining` levels into immediate subdirectories.
fn push_instruction_paths_for_dir(paths: &mut Vec<PathBuf>, dir: &Path, remaining: u32) {
    // Claude-first at this level
    paths.push(dir.join(CLAUDE_MD));
    paths.push(dir.join(AGENTS_MD));

    if remaining == 0 {
        return;
    }
    // Recurse into subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                push_instruction_paths_for_dir(paths, &path, remaining - 1);
            }
        }
    }
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

    #[test]
    fn test_project_instruction_walk_includes_cwd_first() {
        let tmp = std::env::temp_dir().join("aemeath_test_walk_cwd");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let paths = project_instruction_walk(&tmp, 2);
        // First two should be cwd-level CLAUDE.md and AGENTS.md
        assert_eq!(paths[0], tmp.join("CLAUDE.md"));
        assert_eq!(paths[1], tmp.join("AGENTS.md"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_project_instruction_walk_includes_parent() {
        let tmp = std::env::temp_dir().join("aemeath_test_walk_parent");
        let _ = std::fs::remove_dir_all(&tmp);
        let child = tmp.join("sub");
        std::fs::create_dir_all(&child).unwrap();
        let paths = project_instruction_walk(&child, 1);
        // Should include child level and parent level
        assert!(paths.contains(&child.join("CLAUDE.md")));
        assert!(paths.contains(&tmp.join("CLAUDE.md")));
        // Child level comes before parent level
        let child_idx = paths.iter().position(|p| p == &child.join("CLAUDE.md")).unwrap();
        let parent_idx = paths.iter().position(|p| p == &tmp.join("CLAUDE.md")).unwrap();
        assert!(child_idx < parent_idx);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_project_instruction_walk_depth_zero_cwd_only() {
        let tmp = std::env::temp_dir().join("aemeath_test_walk_zero");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let paths = project_instruction_walk(&tmp, 0);
        // depth=0: only cwd level, no subdirs, no parents
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], tmp.join("CLAUDE.md"));
        assert_eq!(paths[1], tmp.join("AGENTS.md"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
