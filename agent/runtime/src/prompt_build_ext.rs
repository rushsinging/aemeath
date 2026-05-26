//! Prompt 构建辅助函数（从 CLI setup.rs 迁移）。

use crate::api::core::config::Config;
use crate::api::core::skill_ops::Skill;
use crate::api::hook::hook::HookRunner;
use crate::bootstrap;

pub async fn build_static_prompt(
    _cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&Config>,
    hook_runner: &HookRunner,
    prompt_parts: crate::api::prompt_build::SystemPromptParts,
    skills: &tokio::sync::Mutex<std::collections::HashMap<String, Skill>>,
) -> String {
    let skills_guard = skills.lock().await;
    let guidance_config = config_file
        .map(|c| c.models.guidance.clone())
        .unwrap_or_default();
    let instructions_hook = bootstrap::InstructionsLoadedHookRunner(hook_runner);
    let model_guidance = crate::api::prompt::guidance::resolve_guidance_async(
        model,
        &guidance_config,
        reasoning,
        Some(&instructions_hook),
    )
    .await;

    let mut prompt = prompt_parts.static_part;
    prompt.push_str(crate::api::prompt::guidance::UNIVERSAL_EXECUTION_DISCIPLINE);
    append_skills(&mut prompt, &skills_guard);
    append_agent_roles(&mut prompt, config_file);
    if !model_guidance.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&model_guidance);
    }
    prompt
}

fn append_skills(prompt: &mut String, skills_guard: &std::collections::HashMap<String, Skill>) {
    if skills_guard.is_empty() {
        return;
    }
    let skill_list: Vec<String> = skills_guard
        .values()
        .map(|s| {
            let alias_str = if s.aliases.is_empty() {
                String::new()
            } else {
                format!(" (aliases: /{})", s.aliases.join(", /"))
            };
            format!("- `{}{}`: {}", s.name, alias_str, s.description)
        })
        .collect();
    prompt.push_str(&format!(
        "\n\n# Available Skills\nThe following skills can be invoked with the Skill tool:\n{}",
        skill_list.join("\n")
    ));
}

fn append_agent_roles(prompt: &mut String, config_file: Option<&Config>) {
    let Some(cfg) = config_file else {
        return;
    };
    if cfg.agents.roles.is_empty() {
        return;
    }
    let role_lines: Vec<String> = cfg
        .agents
        .roles
        .iter()
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
    prompt.push_str(&format!(
        "\n\n# Available Agent Roles\nThe following agent roles are available for the Agent tool's `role` parameter. Choose the most appropriate role for each task:\n{}\nWhen no role fits, omit the `role` parameter to use the default model.",
        role_lines.join("\n")
    ));
}
