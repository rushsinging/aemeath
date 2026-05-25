use ::runtime::api::bootstrap::spawn_mcp_connect;
use ::runtime::api::core::config::SkillsConfig;
use ::runtime::api::core::mcp_manager::McpConnectionManager;
use ::runtime::api::core::skill::Skill;
use ::runtime::api::core::task::TaskStore;
use ::runtime::api::core::tool::ToolRegistry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(super) struct ChatTooling {
    pub registry: Arc<ToolRegistry>,
    pub skills_map: HashMap<String, Skill>,
    pub skills: Arc<Mutex<HashMap<String, Skill>>>,
    pub mcp_manager: Arc<McpConnectionManager>,
}

pub(super) async fn build_chat_tooling(
    cwd: &Path,
    skills_config: Option<&SkillsConfig>,
    task_store: Arc<TaskStore>,
) -> ChatTooling {
    let skills_map = load_configured_skills(cwd, skills_config);
    log_loaded_skills(&skills_map);
    let skills = Arc::new(Mutex::new(skills_map.clone()));
    let registry = register_chat_tools(task_store, skills.clone());
    let mcp_manager = spawn_mcp_connect(registry.clone(), cwd).await;

    ChatTooling {
        registry,
        skills_map,
        skills,
        mcp_manager,
    }
}

fn load_configured_skills(
    cwd: &Path,
    skills_config: Option<&SkillsConfig>,
) -> HashMap<String, Skill> {
    let skill_dirs = configured_skill_dirs(skills_config);
    ::runtime::api::core::skill::load_all_skills(cwd, &skill_dirs)
}

fn configured_skill_dirs(skills_config: Option<&SkillsConfig>) -> Vec<PathBuf> {
    skills_config
        .map(|config| config.dirs.clone())
        .unwrap_or_default()
}

fn log_loaded_skills(skills_map: &HashMap<String, Skill>) {
    if !skills_map.is_empty() {
        log::info!("[Skills] loaded {} skills", skills_map.len());
    }
}

fn register_chat_tools(
    task_store: Arc<TaskStore>,
    skills: Arc<Mutex<HashMap<String, Skill>>>,
) -> Arc<ToolRegistry> {
    let registry = ToolRegistry::new();
    ::runtime::api::tools::register_all_tools(&registry, task_store, skills);
    ::runtime::api::project::register_worktree_tools(&registry);
    Arc::new(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::runtime::api::core::config::SkillsConfig;

    #[test]
    fn test_configured_skill_dirs_uses_config_dirs() {
        let config = SkillsConfig {
            dirs: vec![PathBuf::from("skills-a"), PathBuf::from("skills-b")],
        };

        let result = configured_skill_dirs(Some(&config));

        assert_eq!(
            result,
            vec![PathBuf::from("skills-a"), PathBuf::from("skills-b")]
        );
    }

    #[test]
    fn test_configured_skill_dirs_uses_empty_without_config() {
        let result = configured_skill_dirs(None);

        assert!(result.is_empty());
    }

    #[test]
    fn test_log_loaded_skills_accepts_empty_map() {
        let skills_map = HashMap::new();

        log_loaded_skills(&skills_map);

        assert!(skills_map.is_empty());
    }
}
