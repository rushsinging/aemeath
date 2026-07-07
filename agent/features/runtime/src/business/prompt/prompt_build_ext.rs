//! Prompt 构建辅助函数（从 CLI setup.rs 迁移）。

use crate::utils::bootstrap;
use hook::api::HookRunner;
use prompt::api::skill::Skill;
use share::config::domain::snapshot::ConfigSnapshot;
use share::i18n::prompt::sections::{agent_roles_footer, agent_roles_header, skills_header};

pub async fn build_static_prompt(
    cwd: &std::path::Path,
    model: &str,
    reasoning: bool,
    config_file: Option<&ConfigSnapshot>,
    hook_runner: &HookRunner,
    prompt_parts: crate::business::prompt::build::SystemPromptParts,
    skills: &tokio::sync::Mutex<std::collections::HashMap<String, Skill>>,
) -> String {
    let skills_guard = skills.lock().await;
    let guidance_config = config_file
        .map(|snap| snap.models().guidance.clone())
        .unwrap_or_default();
    let language = config_file.map(|snap| snap.language()).unwrap_or("en");
    let instructions_hook = bootstrap::InstructionsLoadedHookRunner {
        hook_runner,
        workspace_root: cwd,
    };
    let model_guidance = prompt::api::guidance::resolve_guidance_async(
        model,
        &guidance_config,
        reasoning,
        language,
        Some(&instructions_hook),
    )
    .await;

    let mut prompt = prompt_parts.static_part;
    prompt.push_str(prompt::api::guidance::universal_execution_discipline(
        language,
    ));
    append_skills(&mut prompt, &skills_guard, language);
    append_agent_roles(&mut prompt, config_file, language);
    if !model_guidance.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&model_guidance);
    }
    prompt
}

fn append_skills(
    prompt: &mut String,
    skills_guard: &std::collections::HashMap<String, Skill>,
    lang: &str,
) {
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
    let header = skills_header(lang);
    prompt.push_str(&format!("{}{}", header, skill_list.join("\n")));
}

fn append_agent_roles(prompt: &mut String, config_file: Option<&ConfigSnapshot>, lang: &str) {
    let Some(snap) = config_file else {
        return;
    };
    if snap.agents().roles.is_empty() {
        return;
    }
    let role_lines: Vec<String> = snap
        .agents()
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
    let footer = agent_roles_footer(lang);
    let header = agent_roles_header(lang);
    prompt.push_str(&format!("{}{}{}", header, role_lines.join("\n"), footer));
}
