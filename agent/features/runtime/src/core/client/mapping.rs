use sdk::{
    ClipboardImageView, MemoryConfigView, ReflectionConfigView, ReflectionMemorySuggestionView,
    ReflectionOutputView, SessionSummary, SkillView, WorkspaceContextView, WorkspaceStackEntryView,
};

use prompt::api::skill::Skill;
use share::memory::{MemoryCategory, MemoryLayer};
use storage::api::TaskStatus;

pub(crate) fn memory_config_to_sdk(config: share::config::MemoryConfig) -> MemoryConfigView {
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
    image: crate::utils::image::ProcessedImage,
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

pub(crate) fn reflection_output_to_sdk_with_content(
    output: crate::business::reflection::ReflectionOutput,
    content: String,
    input_tokens: u32,
    output_tokens: u32,
    auto_applied: bool,
) -> ReflectionOutputView {
    ReflectionOutputView {
        content,
        input_tokens,
        output_tokens,
        suggested_memories: output
            .suggested_memories
            .into_iter()
            .map(|memory| ReflectionMemorySuggestionView {
                content: memory.content,
                layer: memory_layer_to_sdk(memory.layer).to_string(),
                category: memory_category_to_sdk(memory.category).to_string(),
                tags: memory.tags,
            })
            .collect(),
        outdated_memories: output.outdated_memories,
        auto_applied,
    }
}

fn memory_layer_to_sdk(layer: MemoryLayer) -> &'static str {
    match layer {
        MemoryLayer::Global => "global",
        MemoryLayer::Project => "project",
    }
}

fn memory_category_to_sdk(category: MemoryCategory) -> &'static str {
    match category {
        MemoryCategory::Fact => "fact",
        MemoryCategory::Decision => "decision",
        MemoryCategory::Preference => "preference",
        MemoryCategory::Pattern => "pattern",
        MemoryCategory::Pitfall => "pitfall",
    }
}

pub(crate) fn session_summary_from_runtime(
    session: crate::business::session::Session,
) -> SessionSummary {
    let preview = session
        .messages
        .iter()
        .find(|m| m.role == share::message::Role::User)
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
    tasks: &[storage::api::Task],
    display_map: &std::collections::HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks
        .iter()
        .filter(|t| t.status != TaskStatus::Deleted)
        .count();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed_count, total)];

    let mut completed: Vec<&storage::api::Task> = Vec::new();
    let mut in_progress: Vec<&storage::api::Task> = Vec::new();
    let mut pending: Vec<&storage::api::Task> = Vec::new();
    for task in tasks {
        match task.status {
            TaskStatus::Completed => completed.push(task),
            TaskStatus::InProgress => in_progress.push(task),
            TaskStatus::Pending => pending.push(task),
            TaskStatus::Deleted => {}
        }
    }
    completed.sort_by_key(|t| t.updated_at);
    in_progress.sort_by_key(|t| t.updated_at);
    pending.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));

    let visible = if total <= max_lines {
        ordered_tasks(completed, in_progress, pending)
    } else {
        select_task_window(completed, in_progress, pending, max_lines)
    };
    let shown_count = visible.len();
    let hidden_count = total.saturating_sub(shown_count);
    for task in visible {
        lines.push(format_task_status_line(task, display_map));
    }
    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }
    lines
}

fn ordered_tasks<'a>(
    completed: Vec<&'a storage::api::Task>,
    in_progress: Vec<&'a storage::api::Task>,
    pending: Vec<&'a storage::api::Task>,
) -> Vec<&'a storage::api::Task> {
    completed
        .into_iter()
        .chain(in_progress)
        .chain(pending)
        .collect()
}

fn select_task_window<'a>(
    completed: Vec<&'a storage::api::Task>,
    in_progress: Vec<&'a storage::api::Task>,
    pending: Vec<&'a storage::api::Task>,
    max_lines: usize,
) -> Vec<&'a storage::api::Task> {
    let mut visible = Vec::with_capacity(max_lines);
    if max_lines == 0 {
        return visible;
    }

    // Priority: completed (most recent N, ascending) → in_progress → pending
    // Reserve at least 1 slot for completed (if any exist)
    let mut completed_len = max_lines
        .saturating_sub(in_progress.len())
        .saturating_sub(pending.len());
    if !completed.is_empty() {
        completed_len = completed_len.max(1);
    }
    let skip = completed.len().saturating_sub(completed_len);
    visible.extend(completed.iter().skip(skip).take(completed_len).copied());
    let remaining = max_lines.saturating_sub(visible.len());
    visible.extend(in_progress.into_iter().take(remaining));
    let remaining = max_lines.saturating_sub(visible.len());
    visible.extend(pending.into_iter().take(remaining));
    visible
}

fn format_task_status_line(
    task: &storage::api::Task,
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
    let blocked_by = format_blocked_by(&task.blocked_by, display_map);
    format!(
        "{} #{} {}{}{}",
        icon, display_id, task.subject, owner, blocked_by
    )
}

fn format_blocked_by(
    blocked_by: &[String],
    display_map: &std::collections::HashMap<String, usize>,
) -> String {
    if blocked_by.is_empty() {
        return String::new();
    }

    let deps = blocked_by
        .iter()
        .map(|id| {
            display_map
                .get(id)
                .map(|display_id| format!("#{}", display_id))
                .unwrap_or_else(|| format!("#{}", id))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(" (blocked by {deps})")
}

pub(super) fn workspace_context_to_sdk(
    workspace: crate::business::session::PersistedWorkspaceContext,
) -> WorkspaceContextView {
    WorkspaceContextView {
        path_base: workspace.path_base.into(),
        workspace_root: workspace.workspace_root.into(),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| WorkspaceStackEntryView {
                path_base: entry.path_base.into(),
                workspace_root: entry.workspace_root.into(),
            })
            .collect(),
    }
}

pub(crate) fn message_to_sdk(message: share::message::Message) -> sdk::ChatMessage {
    sdk::ChatMessage {
        role: match message.role {
            share::message::Role::User => "user".to_string(),
            share::message::Role::Assistant => "assistant".to_string(),
        },
        // share::ContentBlock 与 sdk::ContentBlock 同形（serde 成同一 JSON），经 round-trip 映射。
        content: serde_json::from_value(serde_json::to_value(&message.content).unwrap_or_default())
            .unwrap_or_default(),
        metadata: message.metadata.map(|metadata| sdk::ChatMessageMetadata {
            source: match metadata.source {
                share::message::MessageSource::User => sdk::ChatMessageSource::User,
                share::message::MessageSource::SystemGenerated => {
                    sdk::ChatMessageSource::SystemGenerated
                }
            },
        }),
        // input_id 不来自 share::Message；由 runtime→TUI 边界（UserMessagesAdded 事件）
        // 在 event.rs 处按 (InputId, Message) 元组注入（#507 修复）。
        input_id: None,
    }
}

pub(crate) fn message_from_sdk(message: sdk::ChatMessage) -> share::message::Message {
    let role = match message.role.as_str() {
        "assistant" => share::message::Role::Assistant,
        _ => share::message::Role::User,
    };
    let content =
        serde_json::from_value(serde_json::to_value(&message.content).unwrap_or_default())
            .unwrap_or_else(|_| {
                vec![share::message::ContentBlock::Text {
                    text: String::new(),
                }]
            });
    let metadata = message
        .metadata
        .map(|metadata| share::message::MessageMetadata {
            source: match metadata.source {
                sdk::ChatMessageSource::User => share::message::MessageSource::User,
                sdk::ChatMessageSource::SystemGenerated => {
                    share::message::MessageSource::SystemGenerated
                }
            },
        });
    share::message::Message {
        role,
        content,
        metadata,
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
