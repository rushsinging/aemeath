use crate::prompt::{build_system_prompt_parts, PromptContext};
use crate::run_orchestration::runtime;
use kernel::config::{Config, MemoryConfig};
use kernel::hook::HookRunner;
use kernel::skill::Skill;
use provider::types::SystemBlock;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::Mutex;

pub(super) struct ChatPromptBundle {
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
}

pub(super) async fn build_chat_prompt_bundle(
    cwd: &PathBuf,
    model: &str,
    reasoning: bool,
    config_file: Option<&Config>,
    hook_runner: &HookRunner,
    memory_config: MemoryConfig,
    skills: &Mutex<HashMap<String, Skill>>,
    provider_name: &str,
    model_name: &str,
) -> ChatPromptBundle {
    let prompt_context = PromptContext::new(cwd, Some(provider_name), Some(model_name));
    let prompt_parts =
        build_system_prompt_parts(&prompt_context, hook_runner, &memory_config).await;
    build_chat_prompt_bundle_from_parts(
        cwd,
        model,
        reasoning,
        config_file,
        hook_runner,
        prompt_parts,
        skills,
    )
    .await
}

async fn build_chat_prompt_bundle_from_parts(
    cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&Config>,
    hook_runner: &HookRunner,
    prompt_parts: crate::prompt::SystemPromptParts,
    skills: &Mutex<HashMap<String, Skill>>,
) -> ChatPromptBundle {
    let static_prompt = super::super::prompt::build_static_prompt(
        cwd,
        model,
        reasoning,
        config_file,
        hook_runner,
        prompt_parts.clone(),
        skills,
    )
    .await;
    let system_blocks = vec![
        SystemBlock::cached(static_prompt),
        SystemBlock::dynamic(prompt_parts.dynamic_part),
    ];
    let system_prompt_text = runtime::system_prompt_text(&system_blocks);

    ChatPromptBundle {
        system_blocks,
        system_prompt_text,
        user_context: prompt_parts.claude_md,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::SystemPromptParts;
    use kernel::config::Config;
    use std::path::Path;

    fn prompt_parts() -> SystemPromptParts {
        SystemPromptParts {
            static_part: "static instructions".to_string(),
            dynamic_part: "dynamic context".to_string(),
            claude_md: "project instructions".to_string(),
        }
    }

    #[tokio::test]
    async fn test_build_chat_prompt_bundle_from_parts_creates_cached_and_dynamic_blocks() {
        let hook_runner = HookRunner::empty(".".to_string());
        let skills = Mutex::new(HashMap::new());

        let bundle = build_chat_prompt_bundle_from_parts(
            Path::new("."),
            "model-id",
            false,
            None,
            &hook_runner,
            prompt_parts(),
            &skills,
        )
        .await;

        assert_eq!(bundle.system_blocks.len(), 2);
        assert!(bundle.system_prompt_text.contains("static instructions"));
        assert!(bundle.system_prompt_text.contains("dynamic context"));
    }

    #[tokio::test]
    async fn test_build_chat_prompt_bundle_from_parts_preserves_user_context() {
        let hook_runner = HookRunner::empty(".".to_string());
        let skills = Mutex::new(HashMap::new());

        let bundle = build_chat_prompt_bundle_from_parts(
            Path::new("."),
            "model-id",
            false,
            None,
            &hook_runner,
            prompt_parts(),
            &skills,
        )
        .await;

        assert_eq!(bundle.user_context, "project instructions");
    }

    #[tokio::test]
    async fn test_build_chat_prompt_bundle_from_parts_appends_agent_roles_from_config() {
        let hook_runner = HookRunner::empty(".".to_string());
        let skills = Mutex::new(HashMap::new());
        let mut config = Config::default();
        config.agents.roles.insert(
            "reviewer".to_string(),
            kernel::config::AgentRoleConfig {
                description: "reviews code".to_string(),
                model: "provider/model".to_string(),
                ..Default::default()
            },
        );

        let bundle = build_chat_prompt_bundle_from_parts(
            Path::new("."),
            "model-id",
            false,
            Some(&config),
            &hook_runner,
            prompt_parts(),
            &skills,
        )
        .await;

        assert!(bundle.system_prompt_text.contains("reviewer"));
        assert!(bundle.system_prompt_text.contains("reviews code"));
    }
}
