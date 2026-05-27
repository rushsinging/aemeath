use sdk::{
    ClipboardImageView, MemoryConfigView, ReflectionConfigView, ReflectionMemorySuggestionView,
    ReflectionOutputView, SessionSummary, SkillView, WorkspaceContextView,
    WorkspaceStackEntryView,
};

use crate::api::core::task::TaskStatus;
use crate::api::prompt::skill::Skill;

pub(crate) fn memory_config_to_sdk(
    config: crate::api::core::config::MemoryConfig,
) -> MemoryConfigView {
    MemoryConfigView {
        enabled: config.enabled,
        max_entries: config.max_entries,
        similarity_threshold: config.similarity_threshold as f32,
        reflection: ReflectionConfigView {
            enabled: config.reflection.enabled,
            interval_turns: config.reflection.interval_turns,
            auto_apply_suggestions: config.reflection.auto_apply_suggestions,
        },
    }
}

pub(crate) fn skill_to_sdk(skill: Skill) -> SkillView {
    SkillView {
        name: skill.name,
        aliases: skill.aliases,
        description: Some(skill.description),
        content: skill.content,
        source: Some(skill.source_path.display().to_string()),
    }
}

pub(crate) fn processed_image_to_sdk(
    image: crate::api::image::ProcessedImage,
) -> ClipboardImageView {
    ClipboardImageView {
        base64: image.base64,
        media_type: image.media_type,
        final_size: image.final_size,
        display_path: None,
        width: None,
        height: None,
    }
}

pub(crate) fn reflection_output_to_sdk(
    output: crate::api::reflection::ReflectionOutput,
    input_tokens: u32,
    output_tokens: u32,
) -> ReflectionOutputView {
    ReflectionOutputView {
        content: crate::api::reflection::ReflectionEngine::format_output(&output),
        input_tokens,
        output_tokens,
        suggested_memories: output
            .suggested_memories
            .into_iter()
            .map(|memory| ReflectionMemorySuggestionView {
                content: memory.content,
                layer: format!("{:?}", memory.category).to_lowercase(),
            })
            .collect(),
        outdated_memories: output.outdated_memories,
    }
}

pub(crate) fn session_summary_from_runtime(session: crate::business::session::Session) -> SessionSummary {
    let preview = session
        .messages
        .iter()
        .find(|m| m.role == crate::api::core::message::Role::User)
        .map(|m| m.text_content())
        .and_then(|text| {
            let first_line = text.lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                None
            } else {
                Some(first_line.chars().take(50).collect())
            }
        });
    let summary = session.summary();
    SessionSummary {
        id: session.id,
        title: session.metadata.title,
        project: session.metadata.project,
        model: session.metadata.model,
        created_at: session.created_at,
        updated_at: session.updated_at,
        message_count: session.messages.len(),
        preview,
        summary,
    }
}

pub(crate) fn task_status_lines(
    tasks: &[crate::api::core::task::Task],
    display_map: &std::collections::HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed_count, total)];

    let mut completed: Vec<&crate::api::core::task::Task> = Vec::new();
    let mut in_progress: Vec<&crate::api::core::task::Task> = Vec::new();
    let mut pending: Vec<&crate::api::core::task::Task> = Vec::new();
    for task in tasks {
        match task.status {
            TaskStatus::Completed => completed.push(task),
            TaskStatus::InProgress => in_progress.push(task),
            TaskStatus::Pending => pending.push(task),
            TaskStatus::Deleted => {}
        }
    }
    completed.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    in_progress.sort_by_key(|t| t.updated_at);
    pending.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));

    let ordered: Vec<_> = completed
        .into_iter()
        .chain(in_progress)
        .chain(pending)
        .collect();
    let shown_count = ordered.len().min(max_lines);
    let hidden_count = ordered.len() - shown_count;
    for task in ordered.iter().take(shown_count) {
        lines.push(format_task_status_line(task, display_map));
    }
    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }
    lines
}

fn format_task_status_line(
    task: &crate::api::core::task::Task,
    display_map: &std::collections::HashMap<String, usize>,
) -> String {
    let icon = match task.status {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        TaskStatus::Deleted => "?",
    };
    let display_id = display_map.get(&task.id).copied().unwrap_or(0);
    let owner = task
        .owner
        .as_deref()
        .map(|owner| format!(" (@{})", owner))
        .unwrap_or_default();
    format!("{} #{} {}{}", icon, display_id, task.subject, owner)
}

pub(super) fn workspace_context_to_sdk(
    workspace: crate::business::session::WorkspaceContext,
) -> WorkspaceContextView {
    WorkspaceContextView {
        path_base: workspace.path_base.into(),
        working_root: workspace.working_root.into(),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| WorkspaceStackEntryView {
                path_base: entry.path_base.into(),
                working_root: entry.working_root.into(),
            })
            .collect(),
    }
}

pub(crate) fn message_to_sdk(message: crate::api::core::message::Message) -> sdk::ChatMessage {
    sdk::ChatMessage {
        role: match message.role {
            crate::api::core::message::Role::User => "user".to_string(),
            crate::api::core::message::Role::Assistant => "assistant".to_string(),
        },
        content: serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
    }
}

pub(crate) fn message_from_sdk(message: sdk::ChatMessage) -> crate::api::core::message::Message {
    let role = match message.role.as_str() {
        "assistant" => crate::api::core::message::Role::Assistant,
        _ => crate::api::core::message::Role::User,
    };
    let content = serde_json::from_value(message.content).unwrap_or_else(|_| {
        vec![crate::api::core::message::ContentBlock::Text {
            text: String::new(),
        }]
    });
    crate::api::core::message::Message { role, content }
}

/// 将 runtime CommandResult 映射为 SDK 版本。
pub(crate) fn map_command_result(result: crate::api::command::CommandResult) -> sdk::CommandResult {
    match result {
        crate::api::command::CommandResult::Success(msg) => sdk::CommandResult::Success(msg),
        crate::api::command::CommandResult::Error(msg) => sdk::CommandResult::Error(msg),
        crate::api::command::CommandResult::Action(action) => {
            sdk::CommandResult::Action(map_command_action(action))
        }
        crate::api::command::CommandResult::Confirm { message, action } => {
            sdk::CommandResult::Confirm {
                message,
                action: map_confirm_action(action),
            }
        }
    }
}

fn map_command_action(action: crate::api::command::CommandAction) -> sdk::CommandAction {
    use crate::api::command::CommandAction as Rt;
    match action {
        Rt::Exit => sdk::CommandAction::Exit,
        Rt::Clear => sdk::CommandAction::Clear,
        Rt::Compact => sdk::CommandAction::Compact,
        Rt::ResumeSession(id) => sdk::CommandAction::ResumeSession(id),
        Rt::NewSession => sdk::CommandAction::NewSession,
        Rt::ChangeMode(mode) => sdk::CommandAction::ChangeMode(mode),
        Rt::SwitchModel {
            provider_name,
            model_id,
            model_name,
            base_url,
            api_key,
            api_type,
            max_tokens,
            context_window,
            reasoning,
        } => sdk::CommandAction::SwitchModel {
            provider_name,
            model_id,
            model_name,
            base_url,
            api_key,
            api_type,
            max_tokens,
            context_window,
            reasoning,
        },
        Rt::InjectMessage(msg) => sdk::CommandAction::InjectMessage(msg),
        Rt::RunSkill(content) => sdk::CommandAction::RunSkill(content),
        Rt::SetThinking(desired) => sdk::CommandAction::SetThinking(desired),
    }
}

fn map_confirm_action(action: crate::api::command::ConfirmAction) -> sdk::ConfirmAction {
    use crate::api::command::ConfirmAction as Rt;
    match action {
        Rt::DeleteSession(id) => sdk::ConfirmAction::DeleteSession(id),
        Rt::ClearAllHistory => sdk::ConfirmAction::ClearAllHistory,
        Rt::ResetConfig => sdk::ConfirmAction::ResetConfig,
        Rt::ClearCostHistory => sdk::ConfirmAction::ClearCostHistory,
    }
}

pub(crate) fn model_display(source_key: &str, model_name: &str, model_id: &str) -> String {
    let display_name = if model_name.is_empty() {
        model_id
    } else {
        model_name
    };
    format!("{}/{}", source_key, display_name)
}
