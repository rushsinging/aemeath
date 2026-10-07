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
    // #252 PR3：后台任务特性说明（仅阈值启用时注入；显式 0 = 禁用后台化
    // 的会话不注入）。统一模型 / 占位语义 / 查询与停止用法 / sequential 提示。
    let background_threshold = config_file
        .map(|snap| snap.tool_background_threshold_secs())
        .unwrap_or(0);
    if background_threshold > 0 {
        prompt.push_str("\n\n");
        prompt.push_str(background_tasks_guidance_section(language));
    }
    prompt
}

/// 后台任务特性 system prompt 段（#252 D6 / 设计 §8，双语）。
pub(crate) fn background_tasks_guidance_section(lang: &str) -> &'static str {
    match lang {
        "zh" => {
            "# 后台任务\n\
            超过前台等待阈值（当前会话已启用）的 tool call 会自动转后台运行：你会先收到一条占位结果（标注任务已转后台、非终态），任务完成会主动通知你并回注结果，届时可继续处理。\n\
            - 用 BackgroundTaskList / BackgroundTaskStatus / BackgroundTaskLogs 查询任务列表、状态与日志（Logs 支持增量游标），用 BackgroundTaskStop 请求停止。\n\
            - sequential-only 工具的前序调用转后台后，同轮后续命令可能与未完成的前序并行；有顺序依赖时应等待完成通知或先查询状态。"
        }
        _ => {
            "# Background tasks\n\
            Tool calls that exceed the foreground waiting threshold (enabled in this session) are automatically moved to the background: you first receive a placeholder result (marked as moved to the background, not final), and you will be notified when the task completes.\n\
            - Use BackgroundTaskList / BackgroundTaskStatus / BackgroundTaskLogs to list tasks, check status, or read logs (Logs supports an incremental cursor); use BackgroundTaskStop to request a stop.\n\
            - When a sequential-only predecessor has been moved to the background, later commands in the same round may run concurrently with it; if order matters, wait for the completion notification or query the status first."
        }
    }
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
