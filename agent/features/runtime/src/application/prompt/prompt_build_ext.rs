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
            // R3：标注绑定模型在 config.models 中的可用性，让 LLM 可预判。
            // 回退口径与 AgentsConfig::resolve_agent 一致：空 instance.model → default_model。
            let effective_model = if instance.model.trim().is_empty() {
                agents.default_model.as_str()
            } else {
                instance.model.as_str()
            };
            let availability = if effective_model.trim().is_empty() {
                " [no model configured]".to_string()
            } else if snap.models().find_model(effective_model).is_none() {
                format!(" [model unavailable: {effective_model}]")
            } else {
                String::new()
            };
            format!(
                "- `{}` [{}]{}{}{}",
                name, instance.role, desc, model_info, availability
            )
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
#[path = "prompt_build_ext_tests.rs"]
mod tests;
