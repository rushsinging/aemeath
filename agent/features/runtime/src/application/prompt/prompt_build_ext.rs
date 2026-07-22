//! Prompt 构建辅助函数（从 CLI setup.rs 迁移）。

use crate::application::startup as bootstrap;
use hook::HookPort;
use share::config::domain::snapshot::ConfigSnapshot;
use share::i18n::prompt::sections::{agent_roles_footer, agent_roles_header};
use std::sync::Arc;

pub async fn build_static_prompt(
    cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&ConfigSnapshot>,
    hook_port: &Arc<dyn HookPort>,
    prompt_parts: crate::application::prompt::build::SystemPromptParts,
) -> String {
    let guidance_config = config_file
        .map(|snap| snap.models().guidance.clone())
        .unwrap_or_default();
    let language = config_file.map(|snap| snap.language()).unwrap_or("en");
    let instructions_hook = bootstrap::InstructionsLoadedHook {
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
    prompt.push_str(context::guidance::universal_execution_discipline(language));
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
    let role_lines: Vec<String> = snap
        .agents()
        .roles
        .iter()
        .filter(|(_, role)| role.enabled)
        .map(|(name, role)| {
            let desc = if role.description.is_empty() {
                String::new()
            } else {
                format!(": {}", role.description)
            };
            let model_info = if role.model.is_empty() {
                String::new()
            } else {
                format!(" (model: {})", role.model)
            };
            format!("- `{}`{}{}", name, desc, model_info)
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
    use share::config::AgentRoleConfig;
    use share::config::Config;
    use std::collections::HashMap;

    /// 构造一个 ConfigSnapshot，其中 `agents.roles` 与 `language` 按参数设置。
    /// 其余字段使用 `Config::default()`，不触碰文件系统。
    fn make_snapshot(roles: HashMap<String, AgentRoleConfig>, language: &str) -> ConfigSnapshot {
        let mut config = Config::default();
        config.agents.roles = roles;
        config.language = language.to_string();
        ConfigSnapshot::new(config)
    }

    // ── append_agent_roles ────────────────────────────────────

    /// ConfigSnapshot 含 2 个 agent roles（coder + reviewer，带 description 与 model），
    /// 调 append_agent_roles 后 prompt 应包含 role 名 / description / model。
    #[test]
    fn test_append_agent_roles_with_snapshot() {
        // Arrange
        let mut roles = HashMap::new();
        roles.insert(
            "coder".to_string(),
            AgentRoleConfig {
                model: "deepseek/deepseek-chat".to_string(),
                description: "Writes and edits code".to_string(),
                ..Default::default()
            },
        );
        roles.insert(
            "reviewer".to_string(),
            AgentRoleConfig {
                model: "anthropic/claude-sonnet-4".to_string(),
                description: "Reviews code for quality".to_string(),
                ..Default::default()
            },
        );
        let snap = make_snapshot(roles, "en");
        let mut prompt = String::new();

        // Act
        append_agent_roles(&mut prompt, Some(&snap), "en");

        // Assert — role 名、description、model 都应出现在 prompt 中
        assert!(prompt.contains("`coder`"), "应包含 role 名 coder");
        assert!(prompt.contains("`reviewer`"), "应包含 role 名 reviewer");
        assert!(
            prompt.contains("Writes and edits code"),
            "应包含 coder 的 description"
        );
        assert!(
            prompt.contains("Reviews code for quality"),
            "应包含 reviewer 的 description"
        );
        assert!(
            prompt.contains("deepseek/deepseek-chat"),
            "应包含 coder 的 model"
        );
        assert!(
            prompt.contains("anthropic/claude-sonnet-4"),
            "应包含 reviewer 的 model"
        );
    }

    /// ConfigSnapshot 含 agents.roles 空 HashMap，调 append_agent_roles 后
    /// prompt 不应包含任何 role 段（保持为空）。
    #[test]
    fn test_append_agent_roles_empty_snapshot() {
        // Arrange
        let snap = make_snapshot(HashMap::new(), "en");
        let mut prompt = String::from("base");

        // Act
        append_agent_roles(&mut prompt, Some(&snap), "en");

        // Assert — 空 roles 时函数应提前返回，prompt 不追加任何内容
        assert_eq!(prompt, "base", "空 roles 时 prompt 不应追加任何 role 段");
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

    /// disabled role 即使保留定义，也不得把它注入主 LLM。
    #[test]
    fn test_append_agent_roles_omits_disabled_role() {
        let mut roles = HashMap::new();
        roles.insert(
            "coder".to_string(),
            AgentRoleConfig {
                enabled: false,
                description: "编写代码".to_string(),
                ..Default::default()
            },
        );
        roles.insert(
            "reviewer".to_string(),
            AgentRoleConfig {
                description: "审查代码".to_string(),
                ..Default::default()
            },
        );
        let snap = make_snapshot(roles, "zh");
        let mut prompt = String::from("base");

        append_agent_roles(&mut prompt, Some(&snap), "zh");

        assert!(!prompt.contains("`coder`"));
        assert!(prompt.contains("`reviewer`"));
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
            AgentRoleConfig {
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
