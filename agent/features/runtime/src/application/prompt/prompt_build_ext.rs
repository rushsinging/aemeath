//! Prompt 构建辅助函数（从 CLI setup.rs 迁移）。

use crate::application::prompt::instructions_hook::PromptInstructionsHook;
use hook::HookDispatcher;
use share::config::domain::snapshot::ConfigSnapshot;
use share::i18n::prompt::sections::{agent_roles_footer, agent_roles_header};
use std::sync::Arc;

pub async fn build_static_prompt(
    cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&ConfigSnapshot>,
    hook_port: &Arc<dyn HookDispatcher>,
    prompt_parts: crate::application::prompt::build::SystemPromptParts,
) -> String {
    let guidance_config = config_file
        .map(|snap| snap.models().guidance.clone())
        .unwrap_or_default();
    let language = config_file.map(|snap| snap.language()).unwrap_or("en");
    let instructions_hook = PromptInstructionsHook {
        hooks: hook_port.clone(),
        workspace_root: cwd.to_path_buf(),
    };
    let model_guidance = context::guidance::resolve_guidance_async(
        model,
        &guidance_config,
        reasoning,
        language,
        Some(&instructions_hook),
    )
    .await;

    let mut prompt = prompt_parts.static_part;
    append_agent_roles(&mut prompt, config_file, language);
    if !model_guidance.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&model_guidance);
    }
    prompt
}

fn append_agent_roles(prompt: &mut String, config_file: Option<&ConfigSnapshot>, lang: &str) {
    let Some(snap) = config_file else {
        return;
    };
    let agents = snap.agents();
    let merged_roles = agents.merged_roles();
    // 枚举具名实例（names）；描述取实例值，role 描述仅作 fallback。
    let role_lines: Vec<String> = agents
        .names
        .iter()
        .filter(|(_, instance)| instance.enabled)
        .map(|(name, instance)| {
            let description = if instance.description.is_empty() {
                merged_roles
                    .get(&instance.role)
                    .map(|role| role.description.as_str())
                    .unwrap_or("")
            } else {
                instance.description.as_str()
            };
            let desc = if description.is_empty() {
                String::new()
            } else {
                format!(": {}", description)
            };
            let model_info = if instance.model.is_empty() {
                String::new()
            } else {
                format!(" (model: {})", instance.model)
            };
            format!("- `{}` [{}]{}{}", name, instance.role, desc, model_info)
        })
        .collect();
    if role_lines.is_empty() {
        return;
    }
    let footer = agent_roles_footer(lang);
    let header = agent_roles_header(lang);
    prompt.push_str(&format!("{}{}{}", header, role_lines.join("\n"), footer));
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::AgentInstanceConfig;
    use share::config::Config;
    use share::i18n::prompt::discipline::universal_execution_discipline;
    use std::collections::HashMap;

    /// 构造一个 ConfigSnapshot，其中 `agents.names` 与 `language` 按参数设置。
    /// 其余字段使用 `Config::default()`，不触碰文件系统。
    fn make_snapshot(
        names: HashMap<String, AgentInstanceConfig>,
        language: &str,
    ) -> ConfigSnapshot {
        let mut config = Config::default();
        config.agents.names = names;
        config.language = language.to_string();
        share::config::domain::snapshot::ConfigSnapshot::new(config)
    }

    #[tokio::test]
    async fn build_static_prompt_does_not_embed_execution_discipline() {
        let hook_port: Arc<dyn HookDispatcher> = hook::wire_hook_dispatcher(
            &share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config::default()),
        )
        .unwrap();
        let prompt = build_static_prompt(
            std::path::Path::new("/tmp/project"),
            "fake/model",
            false,
            None,
            &hook_port,
            crate::application::prompt::build::SystemPromptParts {
                static_part: "core-system".to_string(),
                initial_git_context: String::new(),
                claude_md: String::new(),
            },
        )
        .await;

        assert!(prompt.contains("core-system"));
        assert!(!prompt.contains(universal_execution_discipline("en")));
    }

    // ── append_agent_roles ────────────────────────────────────

    /// ConfigSnapshot 含 2 个具名实例（coder-fast + reviewer-glm，带 description 与 model），
    /// 调 append_agent_roles 后 prompt 应包含实例名 / description / model。
    #[test]
    fn test_append_agent_roles_with_snapshot() {
        // Arrange
        let mut names = HashMap::new();
        names.insert(
            "coder-fast".to_string(),
            AgentInstanceConfig {
                role: "coder".to_string(),
                model: "deepseek/deepseek-chat".to_string(),
                description: "Writes and edits code".to_string(),
                ..Default::default()
            },
        );
        names.insert(
            "reviewer-glm".to_string(),
            AgentInstanceConfig {
                role: "reviewer".to_string(),
                model: "anthropic/claude-sonnet-4".to_string(),
                description: "Reviews code for quality".to_string(),
                ..Default::default()
            },
        );
        let snap = make_snapshot(names, "en");
        let mut prompt = String::new();

        // Act
        append_agent_roles(&mut prompt, Some(&snap), "en");

        // Assert — 实例名、description、model 都应出现在 prompt 中
        assert!(
            prompt.contains("`coder-fast` [coder]"),
            "应包含实例名与职能（coder-fast [coder]）"
        );
        assert!(
            prompt.contains("`reviewer-glm`"),
            "应包含实例名 reviewer-glm"
        );
        assert!(
            prompt.contains("Writes and edits code"),
            "应包含 coder-fast 的 description"
        );
        assert!(
            prompt.contains("Reviews code for quality"),
            "应包含 reviewer-glm 的 description"
        );
        assert!(
            prompt.contains("deepseek/deepseek-chat"),
            "应包含 coder-fast 的 model"
        );
        assert!(
            prompt.contains("anthropic/claude-sonnet-4"),
            "应包含 reviewer-glm 的 model"
        );
    }

    /// 实例描述为空时回退引用职能的描述（内置 reviewer 的描述填充）。
    #[test]
    fn test_append_agent_roles_falls_back_to_role_description() {
        let mut names = HashMap::new();
        names.insert(
            "reviewer-ds".to_string(),
            AgentInstanceConfig {
                role: "reviewer".to_string(),
                model: "x/y".to_string(),
                ..Default::default()
            },
        );
        let snap = make_snapshot(names, "en");
        let mut prompt = String::new();

        append_agent_roles(&mut prompt, Some(&snap), "en");

        assert!(
            prompt.contains("Read-only review"),
            "内置 reviewer 职能描述应作为实例描述 fallback"
        );
    }

    /// 空 names 时无任何可派发实例，prompt 不追加任何内容——内置职能
    /// 不隐式注入（派发必须命中具名实例）。
    #[test]
    fn test_append_agent_roles_empty_names_appends_nothing() {
        let snap = make_snapshot(HashMap::new(), "en");
        let mut prompt = String::from("base");

        append_agent_roles(&mut prompt, Some(&snap), "en");

        assert_eq!(prompt, "base", "空 names 时 prompt 不应追加任何 role 段");
    }

    /// config_file 为 None 时，append_agent_roles 应直接返回，不追加任何内容。
    #[test]
    fn test_append_agent_roles_none_snapshot() {
        // Arrange
        let mut prompt = String::from("base");

        // Act
        append_agent_roles(&mut prompt, None, "en");

        // Assert
        assert_eq!(
            prompt, "base",
            "config_file 为 None 时 prompt 不应追加任何内容"
        );
    }

    /// disabled 实例即使保留定义，也不得把它注入主 LLM。
    #[test]
    fn test_append_agent_roles_omits_disabled_role() {
        let mut names = HashMap::new();
        names.insert(
            "coder-fast".to_string(),
            AgentInstanceConfig {
                role: "coder".to_string(),
                enabled: false,
                description: "编写代码".to_string(),
                ..Default::default()
            },
        );
        names.insert(
            "reviewer-glm".to_string(),
            AgentInstanceConfig {
                role: "reviewer".to_string(),
                description: "审查代码".to_string(),
                ..Default::default()
            },
        );
        let snap = make_snapshot(names, "zh");
        let mut prompt = String::from("base");

        append_agent_roles(&mut prompt, Some(&snap), "zh");

        assert!(!prompt.contains("`coder-fast`"));
        assert!(prompt.contains("`reviewer-glm`"));
    }

    /// ConfigSnapshot.language="zh" 且 lang 参数传 "zh" 时，
    /// append_agent_roles 应使用中文 header/footer，prompt 中应出现中文 description。
    /// 此测试验证 language 被正确传递给 i18n header/footer（build_static_prompt
    /// 从 snap.language() 读取后传入本函数的 lang 参数）。
    #[test]
    fn test_append_agent_roles_with_snapshot_language_zh() {
        // Arrange — language=zh，验证 lang 参数正确驱动 i18n 文案
        let mut roles = HashMap::new();
        roles.insert(
            "coder".to_string(),
            AgentInstanceConfig {
                description: "编写代码".to_string(),
                ..Default::default()
            },
        );
        let snap = make_snapshot(roles, "zh");
        let mut prompt = String::new();

        // Act
        append_agent_roles(&mut prompt, Some(&snap), "zh");

        // Assert — language=zh 时 role 名与中文 description 应出现
        assert!(prompt.contains("`coder`"), "应包含 role 名 coder");
        assert!(
            prompt.contains("编写代码"),
            "应包含中文 description（language=zh 已正确传递）"
        );
    }
}
